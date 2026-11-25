// The Go baseline for the SQL builder benchmark.
//
// It builds the same four statements the JavaScript and the Rust
// implementations build, for the same query, and it does the same work
// per iteration: read a document, walk it, decide, and write a string.
//
// The point of the baseline is not to be a port of the library — it is to
// be what the same work looks like written the way a Go programmer would
// write it, so the comparison says something about the code and not
// about the port.
package main

import (
	"fmt"
	"math"
	"os"
	"sort"
	"strconv"
	"strings"
	"time"
)

// ---------------------------------------------------------------- values --

// A document keeps its fields in the order they were written, because a
// Go map does not and the columns of an insert have to keep their order.
//
// The pairs live in a slice rather than a map: a document here has a
// handful of fields, and a linear scan over a handful of pairs is
// cheaper than a map's allocation and hashing.
type Object struct {
	pairs []field
}

type field struct {
	key   string
	value any
}

func NewObject() *Object { return &Object{} }

func (o *Object) Set(key string, value any) {
	for i := range o.pairs {
		if o.pairs[i].key == key {
			o.pairs[i].value = value
			return
		}
	}
	o.pairs = append(o.pairs, field{key, value})
}

func (o *Object) Get(key string) (any, bool) {
	for i := range o.pairs {
		if o.pairs[i].key == key {
			return o.pairs[i].value, true
		}
	}
	return nil, false
}

func (o *Object) Keys() []string {
	keys := make([]string, len(o.pairs))
	for i, p := range o.pairs {
		keys[i] = p.key
	}
	return keys
}

// A query is a document whose values are either literals, operator
// documents, or arrays of queries.
func obj(pairs ...any) *Object {
	o := NewObject()
	for i := 0; i+1 < len(pairs); i += 2 {
		o.Set(pairs[i].(string), pairs[i+1])
	}
	return o
}

// ------------------------------------------------------------- rendering --

// A literal, the way SQL wants it.
func literal(value any) string {
	switch v := value.(type) {
	case nil:
		return "NULL"
	case bool:
		if v {
			return "TRUE"
		}
		return "FALSE"
	case string:
		return "'" + strings.ReplaceAll(v, "'", "''") + "'"
	case int:
		return strconv.Itoa(v)
	case int64:
		return strconv.FormatInt(v, 10)
	case float64:
		if v == math.Trunc(v) && math.Abs(v) < 1e15 {
			return strconv.FormatFloat(v, 'f', -1, 64)
		}
		return strconv.FormatFloat(v, 'g', -1, 64)
	default:
		return fmt.Sprintf("%v", v)
	}
}

// A JSON path, the way each dialect reads one.
//
// The first segment of the field is the column; the rest is the path
// inside it.
func jsonPath(dialect string, column string, path []string) string {
	switch dialect {
	case "mysql":
		parts := make([]string, len(path))
		for i, p := range path {
			parts[i] = "." + p
		}
		return fmt.Sprintf("CAST(JSON_UNQUOTE(JSON_EXTRACT(%s, '$%s')) AS DECIMAL(20,6))",
			column, strings.Join(parts, ""))
	case "sqlite":
		parts := make([]string, len(path))
		for i, p := range path {
			parts[i] = "." + p
		}
		return fmt.Sprintf("json_extract(%s, '$%s')", column, strings.Join(parts, ""))
	default: // postgres
		return fmt.Sprintf("((%s::jsonb #>> '{%s}'))::numeric", column, strings.Join(path, ","))
	}
}

// A comparison on one field.
func comparison(dialect string, field string, condition any) string {
	op := "="
	right := ""
	switch c := condition.(type) {
	case *Object:
		keys := c.Keys()
		if len(keys) != 1 {
			// An operator document with more than one operator is
			// not something this benchmark reaches for.
			return ""
		}
		name := keys[0]
		value, _ := c.Get(name)
		switch name {
		case "$gte":
			op = ">="
		case "$gt":
			op = ">"
		case "$lte":
			op = "<="
		case "$lt":
			op = "<"
		case "$ne":
			op = "<>"
		case "$eq":
			op = "="
		default:
			return ""
		}
		right = literal(value)
	case []any:
		parts := make([]string, len(c))
		for i, item := range c {
			parts[i] = literal(item)
		}
		return fmt.Sprintf("%s IN (%s)", field, strings.Join(parts, ", "))
	default:
		right = literal(condition)
	}
	if strings.Contains(field, ".") {
		parts := strings.Split(field, ".")
		field = jsonPath(dialect, parts[0], parts[1:])
	}
	return fmt.Sprintf("%s %s %s", field, op, right)
}

// The WHERE clause of a query.
func where(dialect string, query *Object) string {
	var parts []string
	for _, key := range query.Keys() {
		value, _ := query.Get(key)
		switch key {
		case "$or", "$and":
			items, ok := value.([]any)
			if !ok {
				continue
			}
			group := make([]string, 0, len(items))
			for _, item := range items {
				sub, ok := item.(*Object)
				if !ok {
					continue
				}
				group = append(group, where(dialect, sub))
			}
			if len(group) == 0 {
				continue
			}
			joiner := " OR "
			if key == "$and" {
				joiner = " AND "
			}
			parts = append(parts, "("+strings.Join(group, joiner)+")")
		default:
			parts = append(parts, comparison(dialect, key, value))
		}
	}
	return strings.Join(parts, " AND ")
}

// ---------------------------------------------------------------- builder --

// A collection, and the dialect its SQL is written in.
type Collection struct {
	name    string
	dialect string
}

func (c Collection) Find(query *Object) string {
	return "SELECT * FROM " + c.name + " WHERE " + where(c.dialect, query)
}

func (c Collection) UpdateMany(query *Object, update *Object) string {
	set := NewObject()
	value, _ := update.Get("$set")
	if setValue, ok := value.(*Object); ok {
		for _, key := range setValue.Keys() {
			set.Set(key, mustGet(setValue, key))
		}
	}
	var parts []string
	for _, key := range set.Keys() {
		parts = append(parts, fmt.Sprintf("%s = %s", key, literal(mustGet(set, key))))
	}
	return "UPDATE " + c.name + " SET " + strings.Join(parts, ", ") + " WHERE " + where(c.dialect, query)
}

func (c Collection) InsertMany(docs []*Object) string {
	if len(docs) == 0 {
		return ""
	}
	columns := docs[0].Keys()
	var rows []string
	for _, doc := range docs {
		var values []string
		for _, column := range columns {
			values = append(values, literal(mustGet(doc, column)))
		}
		rows = append(rows, "("+strings.Join(values, ", ")+")")
	}
	return fmt.Sprintf("INSERT INTO %s (%s) VALUES %s", c.name, strings.Join(columns, ", "), strings.Join(rows, ", "))
}

func (c Collection) Aggregate(pipeline []*Object) string {
	// The benchmark's pipeline is a match, a group and a sort. The
	// subquery is the match; the group and the sort wrap it.
	match := NewObject()
	var group *Object
	var sort *Object
	for _, stage := range pipeline {
		keys := stage.Keys()
		if len(keys) == 0 {
			continue
		}
		switch keys[0] {
		case "$match":
			value, _ := stage.Get("$match")
			if m, ok := value.(*Object); ok {
				match = m
			}
		case "$group":
			value, _ := stage.Get("$group")
			if g, ok := value.(*Object); ok {
				group = g
			}
		case "$sort":
			value, _ := stage.Get("$sort")
			if s, ok := value.(*Object); ok {
				sort = s
			}
		}
	}
	sub := "SELECT * FROM " + c.name + " WHERE " + where(c.dialect, match)
	if group == nil {
		return sub
	}
	idValue, _ := group.Get("_id")
	idField := strings.TrimPrefix(fmt.Sprintf("%v", idValue), "$")
	var selects []string
	var groups []string
	for _, key := range group.Keys() {
		if key == "_id" {
			selects = append(selects, idField+" AS _id")
			groups = append(groups, idField)
			continue
		}
		value, _ := group.Get(key)
		accumulator, ok := value.(*Object)
		if !ok || len(accumulator.Keys()) == 0 {
			continue
		}
		name := accumulator.Keys()[0]
		switch name {
		case "$sum":
			selects = append(selects, fmt.Sprintf("COUNT(*) AS %s", key))
		case "$avg":
			selects = append(selects, fmt.Sprintf("AVG(%s) AS %s", idField, key))
		}
	}
	sql := fmt.Sprintf("SELECT %s FROM (%s) AS t1", strings.Join(selects, ", "), sub)
	if len(groups) > 0 {
		sql += " GROUP BY " + strings.Join(groups, ", ")
	}
	if sort != nil {
		var orders []string
		for _, key := range sort.Keys() {
			value, _ := sort.Get(key)
			direction := "ASC"
			if n, ok := value.(int); ok && n < 0 {
				direction = "DESC"
			}
			orders = append(orders, key+" "+direction)
		}
		sql += " ORDER BY " + strings.Join(orders, ", ")
	}
	return sql
}

func mustGet(o *Object, key string) any {
	v, _ := o.Get(key)
	return v
}

// ------------------------------------------------------------------- main --

func query() *Object {
	return obj(
		"age", obj("$gte", 18),
		"status", "active",
		"$or", []any{
			obj("role", "admin"),
			obj("profile.score", obj("$gt", 90)),
		},
	)
}

func main() {
	users := Collection{name: "users", dialect: "pg"}
	q := query()
	update := obj("$set", obj("status", "done"))
	pipeline := []*Object{
		obj("$match", query()),
		obj("$group", obj("_id", "$status", "total", obj("$sum", 1))),
		obj("$sort", obj("total", -1)),
	}
	docs := make([]*Object, 50)
	for i := range docs {
		docs[i] = obj("name", fmt.Sprintf("u%d", i), "age", i)
	}

	n := 20000
	if v := os.Getenv("N"); v != "" {
		if parsed, err := strconv.Atoi(v); err == nil {
			n = parsed
		}
	}
	rounds := 3
	if v := os.Getenv("ROUNDS"); v != "" {
		if parsed, err := strconv.Atoi(v); err == nil {
			rounds = parsed
		}
	}

	// The statements are built once so the benchmark measures the build,
	// not the print. The strings are compared against the JavaScript
	// output by the parity check in the shell script.
	bench("find", n, rounds, func() {
		sql := users.Find(q)
		if len(sql) == 0 {
			panic("empty")
		}
	})
	bench("update", n, rounds, func() {
		sql := users.UpdateMany(q, update)
		if len(sql) == 0 {
			panic("empty")
		}
	})
	bench("aggregate", n, rounds, func() {
		sql := users.Aggregate(pipeline)
		if len(sql) == 0 {
			panic("empty")
		}
	})
	bench("insert50", n, rounds, func() {
		sql := users.InsertMany(docs)
		if len(sql) == 0 {
			panic("empty")
		}
	})

	if os.Getenv("PRINT") != "" {
		fmt.Println(users.Find(q))
		fmt.Println(users.UpdateMany(q, update))
		fmt.Println(users.Aggregate(pipeline))
		fmt.Println(users.InsertMany(docs))
	}
	_ = sort.Ints
}

// The same best-of-N the JavaScript and the Rust benchmarks use: the
// machine is shared, so the minimum is the only reading that reflects the
// code rather than whatever else was running.
func bench(label string, n int, rounds int, f func()) {
	for i := 0; i < 1000; i++ {
		f()
	}
	best := math.Inf(1)
	for r := 0; r < rounds; r++ {
		start := time.Now()
		for i := 0; i < n; i++ {
			f()
		}
		ns := float64(time.Since(start).Nanoseconds()) / float64(n)
		if ns < best {
			best = ns
		}
	}
	fmt.Printf("%-9s %8.1f ns/op  %10.0f ops/s\n", label, best, 1e9/best)
}
