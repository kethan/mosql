// The Go baseline for the in-memory engine benchmark.
//
// It stores documents as `map[string]any` — the shape a Go program
// would use for a document read from JSON — and implements the five
// operations the benchmark exercises: a find with a filter, a find with
// a sort and a limit, an aggregate with a match, a group and a sort, an
// update, and an insert.
//
// Numbers are float64 throughout, because that is what a JSON decoder
// produces and what the JavaScript implementation compares with. A Go
// program that kept `int` alongside `float64` would be comparing
// different types, not measuring the same work.
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

// A document.
type Doc map[string]any

// ---------------------------------------------------------------- reading --

// The value at a dotted path, and whether it was there.
func get(doc Doc, path string) (any, bool) {
	parts := strings.Split(path, ".")
	var current any = map[string]any(doc)
	for _, part := range parts {
		object, ok := current.(map[string]any)
		if !ok {
			return nil, false
		}
		current, ok = object[part]
		if !ok {
			return nil, false
		}
	}
	return current, true
}

// Set a value at a dotted path.
func set(doc Doc, path string, value any) {
	parts := strings.Split(path, ".")
	current := map[string]any(doc)
	for _, part := range parts[:len(parts)-1] {
		next, ok := current[part].(map[string]any)
		if !ok {
			next = map[string]any{}
			current[part] = next
		}
		current = next
	}
	current[parts[len(parts)-1]] = value
}

// ------------------------------------------------------------- comparing --

// Compare two values the way a filter does.
func compare(left any, right any) int {
	ln, lok := number(left)
	rn, rok := number(right)
	if lok && rok {
		switch {
		case ln < rn:
			return -1
		case ln > rn:
			return 1
		default:
			return 0
		}
	}
	if ls, ok := left.(string); ok {
		if rs, ok := right.(string); ok {
			return strings.Compare(ls, rs)
		}
	}
	if lb, ok := left.(bool); ok {
		if rb, ok := right.(bool); ok {
			switch {
			case lb == rb:
				return 0
			case !lb:
				return -1
			default:
				return 1
			}
		}
	}
	return strings.Compare(fmt.Sprintf("%v", left), fmt.Sprintf("%v", right))
}

func number(value any) (float64, bool) {
	switch v := value.(type) {
	case float64:
		return v, true
	case int:
		return float64(v), true
	case int64:
		return float64(v), true
	}
	return 0, false
}

// Does a document match a filter?
func matches(doc Doc, filter map[string]any) bool {
	for key, condition := range filter {
		switch key {
		case "$or":
			items, _ := condition.([]any)
			found := false
			for _, item := range items {
				sub, _ := item.(map[string]any)
				if matches(doc, sub) {
					found = true
					break
				}
			}
			if !found {
				return false
			}
		case "$and":
			items, _ := condition.([]any)
			for _, item := range items {
				sub, _ := item.(map[string]any)
				if !matches(doc, sub) {
					return false
				}
			}
		default:
			value, present := get(doc, key)
			if !present {
				return false
			}
			operators, isQuery := condition.(map[string]any)
			if !isQuery {
				if compare(value, condition) != 0 {
					return false
				}
				continue
			}
			for name, wanted := range operators {
				switch name {
				case "$gte":
					if compare(value, wanted) < 0 {
						return false
					}
				case "$gt":
					if compare(value, wanted) <= 0 {
						return false
					}
				case "$lte":
					if compare(value, wanted) > 0 {
						return false
					}
				case "$lt":
					if compare(value, wanted) >= 0 {
						return false
					}
				case "$ne":
					if compare(value, wanted) == 0 {
						return false
					}
				case "$eq":
					if compare(value, wanted) != 0 {
						return false
					}
				default:
					return false
				}
			}
		}
	}
	return true
}

// ------------------------------------------------------------ the engine --

type Collection struct {
	docs []Doc
}

func (c *Collection) Find(filter map[string]any) []Doc {
	var out []Doc
	for _, doc := range c.docs {
		if matches(doc, filter) {
			out = append(out, doc)
		}
	}
	return out
}

func (c *Collection) FindSorted(filter map[string]any, sortSpec map[string]any, limit int) []Doc {
	out := c.Find(filter)
	keys := make([]string, 0, len(sortSpec))
	for key := range sortSpec {
		keys = append(keys, key)
	}
	sort.SliceStable(out, func(i, j int) bool {
		for _, key := range keys {
			direction, _ := number(sortSpec[key])
			left, _ := get(out[i], key)
			right, _ := get(out[j], key)
			c := compare(left, right)
			if c == 0 {
				continue
			}
			if direction < 0 {
				return c > 0
			}
			return c < 0
		}
		return false
	})
	if limit > 0 && len(out) > limit {
		out = out[:limit]
	}
	return out
}

// The result of an aggregate: one document per group.
func (c *Collection) Aggregate(pipeline []map[string]any) []Doc {
	docs := c.docs
	var groupSpec map[string]any
	var sortSpec map[string]any
	for _, stage := range pipeline {
		for name, value := range stage {
			switch name {
			case "$match":
				filter, _ := value.(map[string]any)
				var matched []Doc
				for _, doc := range docs {
					if matches(doc, filter) {
						matched = append(matched, doc)
					}
				}
				docs = matched
			case "$group":
				groupSpec, _ = value.(map[string]any)
			case "$sort":
				sortSpec, _ = value.(map[string]any)
			}
		}
	}
	if groupSpec == nil {
		return docs
	}

	// The groups, in the order they first appear.
	var order []any
	groups := map[string]Doc{}
	for _, doc := range docs {
		idValue, _ := groupSpec["_id"].(string)
		idValue = strings.TrimPrefix(idValue, "$")
		key, _ := get(doc, idValue)
		rendered := fmt.Sprintf("%v", key)
		group, seen := groups[rendered]
		if !seen {
			group = Doc{"_id": key}
			groups[rendered] = group
			order = append(order, key)
		}
		for name, accumulator := range groupSpec {
			if name == "_id" {
				continue
			}
			operators, _ := accumulator.(map[string]any)
			for operator, argument := range operators {
				switch operator {
				case "$sum":
					previous, _ := number(group[name])
					addend := 1.0
					if field, ok := argument.(string); ok {
						if value, found := get(doc, strings.TrimPrefix(field, "$")); found {
							if n, ok := number(value); ok {
								addend = n
							}
						}
					}
					group[name] = previous + addend
				case "$avg":
					count, _ := number(group["__count"])
					sum, _ := number(group["__sum"])
					value, found := get(doc, strings.TrimPrefix(argument.(string), "$"))
					n, ok := number(value)
					if !found || !ok {
						continue
					}
					group["__count"] = count + 1
					group["__sum"] = sum + n
					group[name] = group["__sum"].(float64) / group["__count"].(float64)
				}
			}
		}
	}
	out := make([]Doc, 0, len(order))
	for _, key := range order {
		group := groups[fmt.Sprintf("%v", key)]
		delete(group, "__count")
		delete(group, "__sum")
		out = append(out, group)
	}
	if sortSpec != nil {
		keys := make([]string, 0, len(sortSpec))
		for key := range sortSpec {
			keys = append(keys, key)
		}
		sort.SliceStable(out, func(i, j int) bool {
			for _, key := range keys {
				direction, _ := number(sortSpec[key])
				c := compare(out[i][key], out[j][key])
				if c == 0 {
					continue
				}
				if direction < 0 {
					return c > 0
				}
				return c < 0
			}
			return false
		})
	}
	return out
}

// How many documents a filter matched, and how many were changed.
func (c *Collection) UpdateMany(filter map[string]any, update map[string]any) int {
	matched := 0
	for _, doc := range c.docs {
		if !matches(doc, filter) {
			continue
		}
		matched++
		for name, changes := range update {
			operators, _ := changes.(map[string]any)
			for operator, argument := range operators {
				switch operator {
				case "$set":
					set(doc, name, argument)
				case "$inc":
					previous, _ := get(doc, name)
					n, _ := number(previous)
					by, _ := number(argument)
					set(doc, name, n+by)
				}
			}
		}
	}
	return matched
}

func (c *Collection) Insert(doc Doc) { c.docs = append(c.docs, doc) }

func (c *Collection) Size() int { return len(c.docs) }

// ------------------------------------------------------------------- main --

func document(i int) Doc {
	return Doc{
		"_id":    float64(i),
		"name":   fmt.Sprintf("user%d", i),
		"age":    float64(18 + i%60),
		"active": i%3 == 0,
		"tags":   []any{"t1", "t2"},
		"profile": map[string]any{
			"score": float64((i * 7) % 100),
			"city":  map[bool]string{true: "London", false: "Paris"}[i%2 == 0],
		},
	}
}

func main() {
	n := 10000
	docs := make([]Doc, n)
	for i := range docs {
		docs[i] = document(i)
	}
	coll := &Collection{docs: docs}

	query := map[string]any{
		"age": map[string]any{"$gte": float64(40)},
		"$or": []any{
			map[string]any{"active": true},
			map[string]any{"profile.score": map[string]any{"$gt": float64(80)}},
		},
	}
	pipeline := []map[string]any{
		{"$match": map[string]any{"active": true}},
		{"$group": map[string]any{
			"_id": "$profile.city",
			"n":   map[string]any{"$sum": float64(1)},
			"avg": map[string]any{"$avg": "$age"},
		}},
		{"$sort": map[string]any{"n": float64(-1)}},
	}
	update := map[string]any{
		"$set": map[string]any{"verified": true},
		"$inc": map[string]any{"profile.score": float64(1)},
	}

	rounds := 3
	if v := os.Getenv("ROUNDS"); v != "" {
		if parsed, err := strconv.Atoi(v); err == nil {
			rounds = parsed
		}
	}

	bench("find", 200, rounds, func() {
		if len(coll.Find(query)) == 0 {
			panic("empty")
		}
	})
	bench("find+sort", 200, rounds, func() {
		if len(coll.FindSorted(query, map[string]any{"age": float64(-1)}, 10)) == 0 {
			panic("empty")
		}
	})
	bench("aggregate", 100, rounds, func() {
		if len(coll.Aggregate(pipeline)) == 0 {
			panic("empty")
		}
	})
	bench("updateMany", 100, rounds, func() {
		if coll.UpdateMany(query, update) == 0 {
			panic("no match")
		}
	})
	sink := &Collection{}
	bench("insert1", 2000, rounds, func() {
		sink.Insert(document(999999))
	})

	fmt.Printf("\ncollection size: %d\n", coll.Size())

	// The check mode prints what the engine actually computed, so the
	// Rust and the JavaScript baselines can be compared against it. A
	// timing comparison only means something if the three are doing the
	// same work.
	if os.Getenv("CHECK") != "" {
		matched := len(coll.Find(query))
		groups := coll.Aggregate(pipeline)
		city := ""
		n := 0.0
		avg := 0.0
		if len(groups) > 0 {
			city, _ = groups[0]["_id"].(string)
			n, _ = number(groups[0]["n"])
			avg, _ = number(groups[0]["avg"])
		}
		updated := coll.UpdateMany(query, update)
		fmt.Printf("CHECK find=%d groups=%d city=%s n=%v avg=%v updated=%d\n",
			matched, len(groups), city, n, avg, updated)
	}
}

// The same best-of-N the JavaScript and the Rust benchmarks use.
func bench(label string, n int, rounds int, f func()) {
	for i := 0; i < 3; i++ {
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
	fmt.Printf("%-12s %10.1f ns/op %12.0f ops/s   (%.2f ms/op)\n", label, best, 1e9/best, best/1e6)
}
