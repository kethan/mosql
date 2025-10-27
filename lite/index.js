// ============================================
// MONGODB-TO-SQL LITE VERSION
// Features: JSON paths, CRUD, No Aggregation
// ============================================

const isObject = (obj) => typeof obj === 'object' && obj !== null && !Array.isArray(obj);
const is$ = (str) => typeof str === 'string' && str.startsWith('$');

const PATTERNS = {
    COLUMN: /^[\w.]+$/,
    NON_WORD: /[^\w]/g,
    NUMERIC_INDEX: /\.(\d+)/g,
};

const validate = {
    column: (name) => {
        if (!PATTERNS.COLUMN.test(name)) {
            throw new Error(`Invalid column name: ${name}`);
        }
        return name;
    },
    alias: (str) => str.replace(PATTERNS.NON_WORD, '_'),
    array: (val, op) => {
        if (!Array.isArray(val)) throw new Error(`${op} requires an array`);
        return val;
    },
    int: (val, op) => {
        const num = Number(val);
        if (!Number.isInteger(num) || num < 0) {
            throw new Error(`${op} requires a non-negative integer`);
        }
        return num;
    },
};

const escape = (value, db = 'sqlite') => {
    if (value === null) return 'NULL';
    if (value === undefined) throw new Error('Cannot escape undefined value');

    const type = typeof value;

    if (type === 'number' || type === 'bigint') return String(value);
    if (type === 'boolean') return db === 'pg' ? (value ? 'TRUE' : 'FALSE') : (value ? '1' : '0');

    if (isObject(value) || Array.isArray(value)) {
        const json = JSON.stringify(value).replace(/'/g, "''");
        return db === 'pg' ? `'${json}'::jsonb` : `'${json}'`;
    }

    return `'${String(value).replace(/'/g, "''")}'`;
};

const jsonPath = (path, db = 'sqlite', castTo = 'text') => {
    if (!path) return '';

    const [column, ...rest] = path.split('.');
    if (!rest.length) return validate.column(column);

    const col = validate.column(column);

    if (db === 'pg') {
        const extracted = `(${col}::jsonb #>> '{${rest.join(',')}}')`;
        if (castTo === 'numeric') return `(${extracted})::numeric`;
        if (castTo === 'int') return `(${extracted})::int`;
        return extracted;
    }

    const dollarPath = `$.${rest.join('.').replace(PATTERNS.NUMERIC_INDEX, '[$1]')}`;
    return `json_extract(${col}, '${dollarPath}')`;
};

const jsonUpdate = (path, value, db, op = 'set') => {
    const [column, ...rest] = path.split('.');
    if (!rest.length) return null;

    const col = validate.column(column);
    const pathArray = rest.join(',');
    const dollarPath = `$.${rest.join('.')}`;

    const buildValue = (val) => {
        if (typeof val === 'object') {
            const json = JSON.stringify(val);
            return db === 'pg' ? `'${json}'::jsonb` :
                db === 'mysql' ? `CAST('${json}' AS JSON)` :
                    `'${json}'`;
        }
        return db === 'pg' ? `to_jsonb(${escape(val, db)})` : escape(val, db);
    };

    const extract = {
        pg: `COALESCE((${col}::jsonb #>> '{${pathArray}}')::numeric, ${op === 'mul' ? 1 : 0})`,
        mysql: `COALESCE(JSON_EXTRACT(${col}, '${dollarPath}'), ${op === 'mul' ? 1 : 0})`,
        sqlite: `COALESCE(json_extract(${col}, '${dollarPath}'), ${op === 'mul' ? 1 : 0})`,
    };

    switch (op) {
        case 'remove':
            return db === 'pg' ? `${col} = ${col} #- '{${pathArray}}'` :
                db === 'mysql' ? `${col} = JSON_REMOVE(${col}, '${dollarPath}')` :
                    `${col} = json_remove(${col}, '${dollarPath}')`;

        case 'inc':
        case 'mul': {
            const operator = op === 'inc' ? '+' : '*';
            const expr = `${extract[db] || extract.sqlite} ${operator} ${escape(value, db)}`;

            return db === 'pg' ? `${col} = jsonb_set(${col}::jsonb, '{${pathArray}}', to_jsonb(${expr}), true)` :
                db === 'mysql' ? `${col} = JSON_SET(${col}, '${dollarPath}', ${expr})` :
                    `${col} = json_set(${col}, '${dollarPath}', ${expr})`;
        }

        default: {
            const jsonValue = buildValue(value);

            return db === 'pg' ? `${col} = jsonb_set(COALESCE(${col}, '{}'::jsonb), '{${pathArray}}', ${jsonValue}, true)` :
                db === 'mysql' ? `${col} = JSON_SET(COALESCE(${col}, '{}'), '${dollarPath}', ${jsonValue})` :
                    `${col} = json_set(COALESCE(${col}, '{}'), '${dollarPath}', ${jsonValue})`;
        }
    }
};

const batchJsonUpdate = (fields, db) => {
    const byColumn = {};
    const regular = {};

    Object.entries(fields).forEach(([path, value]) => {
        if (path.includes('.')) {
            const [col] = path.split('.');
            if (!byColumn[col]) byColumn[col] = {};
            byColumn[col][path] = value;
        } else {
            regular[path] = value;
        }
    });

    const updates = [];

    Object.entries(regular).forEach(([key, val]) => {
        updates.push(`${validate.column(key)} = ${escape(val, db)}`);
    });

    Object.entries(byColumn).forEach(([col, paths]) => {
        const entries = Object.entries(paths);

        if (entries.length === 1) {
            updates.push(jsonUpdate(entries[0][0], entries[0][1], db, 'set'));
        } else if (db === 'sqlite' || db === 'mysql') {
            const func = db === 'mysql' ? 'JSON_SET' : 'json_set';
            const pairs = entries.map(([path, val]) => {
                const jsonPath = `$.${path.split('.').slice(1).join('.')}`;
                const value = typeof val === 'object'
                    ? (db === 'mysql' ? `CAST('${JSON.stringify(val)}' AS JSON)` : `'${JSON.stringify(val)}'`)
                    : escape(val, db);
                return `'${jsonPath}', ${value}`;
            }).join(', ');

            updates.push(`${validate.column(col)} = ${func}(COALESCE(${validate.column(col)}, '{}'), ${pairs})`);
        } else {
            let expr = `COALESCE(${validate.column(col)}, '{}'::jsonb)`;
            entries.forEach(([path, val]) => {
                const jsonPath = path.split('.').slice(1).join(',');
                const value = typeof val === 'object'
                    ? `'${JSON.stringify(val)}'::jsonb`
                    : `to_jsonb(${escape(val, db)})`;
                expr = `jsonb_set(${expr}, '{${jsonPath}}', ${value}, true)`;
            });
            updates.push(`${validate.column(col)} = ${expr}`);
        }
    });

    return updates;
};

const filterOps = {
    $eq: (v, db) => `= ${escape(v, db)}`,
    $ne: (v, db) => `!= ${escape(v, db)}`,
    $gt: (v, db) => `> ${escape(v, db)}`,
    $gte: (v, db) => `>= ${escape(v, db)}`,
    $lt: (v, db) => `< ${escape(v, db)}`,
    $lte: (v, db) => `<= ${escape(v, db)}`,

    $in: (v, db) => {
        validate.array(v, '$in');
        return v.length === 0 ? '= 1 AND 1 = 0' : `IN (${v.map(x => escape(x, db)).join(', ')})`;
    },

    $nin: (v, db) => {
        validate.array(v, '$nin');
        return v.length === 0 ? '= 1 OR 1 = 1' : `NOT IN (${v.map(x => escape(x, db)).join(', ')})`;
    },

    $like: (v, db) => `LIKE ${escape(v, db)}`,
    $ilike: (v, db, field) => db === 'pg' ? `ILIKE ${escape(v, db)}` : `LOWER(${field}) LIKE LOWER(${escape(v, db)})`,
    $nlike: (v, db) => `NOT LIKE ${escape(v, db)}`,
    $nilike: (v, db, field) => db === 'pg' ? `NOT ILIKE ${escape(v, db)}` : `LOWER(${field}) NOT LIKE LOWER(${escape(v, db)})`,

    $regex: (v, db) => {
        const pattern = v instanceof RegExp ? v.source : v;
        return (db === 'pg' ? '~' : 'REGEXP') + ` ${escape(pattern, db)}`;
    },

    $exists: (v) => v ? 'IS NOT NULL' : 'IS NULL',
};

const exprOps = {
    $add: (args, ctx) => args.map(a => ctx.expr(a)).join(' + '),
    $subtract: (args, ctx) => args.map(a => ctx.expr(a)).join(' - '),
    $multiply: (args, ctx) => args.map(a => ctx.expr(a)).join(' * '),
    $divide: (args, ctx) => args.map(a => ctx.expr(a)).join(' / '),
    $mod: (args, ctx) => `${ctx.expr(args[0])} % ${ctx.expr(args[1])}`,

    $concat: (args, ctx) => {
        const exprs = args.map(a => ctx.expr(a));
        return ctx.db === 'pg' || ctx.db === 'mysql' ? `CONCAT(${exprs.join(', ')})` : exprs.join(' || ');
    },
    $upper: (args, ctx) => `UPPER(${ctx.expr(args[0])})`,
    $lower: (args, ctx) => `LOWER(${ctx.expr(args[0])})`,
    $substr: (args, ctx) => `SUBSTRING(${ctx.expr(args[0])}, ${ctx.expr(args[1])}, ${ctx.expr(args[2])})`,


    // Aggregates
    $min: (args, ctx) => args.length === 1 ? `MIN(${ctx.expr(args[0])})` : `LEAST(${args.map(a => ctx.expr(a)).join(', ')})`,
    $max: (args, ctx) => args.length === 1 ? `MAX(${ctx.expr(args[0])})` : `GREATEST(${args.map(a => ctx.expr(a)).join(', ')})`,
    $avg: (args, ctx) => `AVG(${ctx.expr(args[0], false, 'numeric')})`,
    $sum: (args, ctx) => `SUM(${ctx.expr(args[0], false, 'numeric')})`,
    $count: () => `COUNT(*)`,

    $eq: (args, ctx) => `${ctx.expr(args[0])} = ${ctx.expr(args[1])}`,
    $ne: (args, ctx) => `${ctx.expr(args[0])} <> ${ctx.expr(args[1])}`,
    $gt: (args, ctx) => `${ctx.expr(args[0])} > ${ctx.expr(args[1])}`,
    $gte: (args, ctx) => `${ctx.expr(args[0])} >= ${ctx.expr(args[1])}`,
    $lt: (args, ctx) => `${ctx.expr(args[0])} < ${ctx.expr(args[1])}`,
    $lte: (args, ctx) => `${ctx.expr(args[0])} <= ${ctx.expr(args[1])}`,


    // Array (for expressions, not filters)
    $in: (args, ctx) => {
        if (args.length !== 2 || !Array.isArray(args[1])) {
            throw new Error('$in expression requires [value, array]');
        }
        return `${ctx.expr(args[0])} IN (${args[1].map(a => ctx.expr(a)).join(', ')})`;
    },
    $nin: (args, ctx) => {
        if (args.length !== 2 || !Array.isArray(args[1])) {
            throw new Error('$nin expression requires [value, array]');
        }
        return `${ctx.expr(args[0])} NOT IN (${args[1].map(a => ctx.expr(a)).join(', ')})`;
    },

    $and: (args, ctx) => args.map(a => `(${ctx.expr(a)})`).join(' AND '),
    $or: (args, ctx) => args.map(a => `(${ctx.expr(a)})`).join(' OR '),
    $not: (args, ctx) => `NOT (${ctx.expr(args[0])})`,

    $cond: (args, ctx) => {
        if (args.length !== 3) throw new Error('$cond requires [condition, then, else]');
        return `CASE WHEN ${ctx.expr(args[0])} THEN ${ctx.expr(args[1])} ELSE ${ctx.expr(args[2])} END`;
    },

    $switch: (args, ctx) => {
        const config = args[0];
        if (!config?.branches || !Array.isArray(config.branches)) {
            throw new Error('$switch requires {branches: [...], default: ...}');
        }
        const branches = config.branches
            .map(b => {
                if (!b.case || !b.then) throw new Error('Each branch needs {case, then}');
                return `WHEN ${ctx.expr(b.case)} THEN ${ctx.expr(b.then)}`;
            })
            .join(' ');
        const defaultVal = config.default !== undefined ? ctx.expr(config.default) : 'NULL';
        return `CASE ${branches} ELSE ${defaultVal} END`;
    },

    $exists: (args, ctx) => {
        if (args.length !== 2) throw new Error('$exists expression requires [field, boolean]');
        return `${ctx.expr(args[0])} ${args[1] ? 'IS NOT NULL' : 'IS NULL'}`;
    },

    // Date/Time
    $dateToString: (args, ctx) => {
        const config = args[0];
        const date = ctx.expr(config.date);
        if (ctx.db === 'pg') {
            return `TO_CHAR(${date}, '${config.format || 'YYYY-MM-DD'}')`;
        }
        return `DATE_FORMAT(${date}, '${config.format || '%Y-%m-%d'}')`;
    },
};

const updateOps = {
    $set: (fields, db) => batchJsonUpdate(fields, db),

    $inc: (fields, db) => Object.entries(fields).map(([key, val]) =>
        key.includes('.') ? jsonUpdate(key, val, db, 'inc') : `${validate.column(key)} = ${validate.column(key)} + ${escape(val, db)}`
    ),

    $mul: (fields, db) => Object.entries(fields).map(([key, val]) =>
        key.includes('.') ? jsonUpdate(key, val, db, 'mul') : `${validate.column(key)} = ${validate.column(key)} * ${escape(val, db)}`
    ),

    $min: (fields, db) => Object.entries(fields).map(([key, val]) => {
        const field = key.includes('.') ? jsonPath(key, db, 'numeric') : validate.column(key);
        const func = db === 'sqlite' ? 'MIN' : 'LEAST';
        return `${field} = ${func}(${field}, ${escape(val, db)})`;
    }),

    $max: (fields, db) => Object.entries(fields).map(([key, val]) => {
        const field = key.includes('.') ? jsonPath(key, db, 'numeric') : validate.column(key);
        const func = db === 'sqlite' ? 'MAX' : 'GREATEST';
        return `${field} = ${func}(${field}, ${escape(val, db)})`;
    }),

    $unset: (fields, db) => Object.keys(fields).map(key =>
        key.includes('.') ? jsonUpdate(key, null, db, 'remove') : `${validate.column(key)} = NULL`
    ),

    $currentDate: (fields, db) => {
        const now = db === 'pg' ? 'CURRENT_TIMESTAMP' : db === 'mysql' ? 'NOW()' : "datetime('now')";
        return Object.entries(fields).map(([key]) =>
            key.includes('.') ? jsonUpdate(key, now, db, 'set') : `${validate.column(key)} = ${now}`
        );
    },
};

const filter = (query, db = 'sqlite') => {
    if (!isObject(query)) return query;

    return Object.entries(query).map(([key, value]) => {
        if (is$(key)) {
            if (key === '$and' || key === '$or') {
                validate.array(value, key);
                const op = key === '$and' ? 'AND' : 'OR';
                return `(${value.map(q => filter(q, db)).join(` ${op} `)})`;
            }
            if (key === '$not') return `NOT (${filter(value, db)})`;
            if (key === '$expr') return expression(value, db);
            throw new Error(`Unknown filter operator: ${key}`);
        }

        const field = key.includes('.') ? jsonPath(key, db) : validate.column(key);

        if (isObject(value)) {
            const [op, val] = Object.entries(value)[0];
            if (!filterOps[op]) throw new Error(`Unknown operator: ${op}`);
            return `${field} ${filterOps[op](val, db, field)}`;
        }

        return `${field} = ${escape(value, db)}`;
    }).join(' AND ');
};

const expression = (expr, db = 'sqlite', asIdentifier = false, castTo = 'text') => {
    if (is$(expr)) {
        const val = expr.slice(1);
        return val.includes('.') ? jsonPath(val, db, castTo) : validate.column(val);
    }

    if (isObject(expr)) {
        const [op, args] = Object.entries(expr)[0];
        if (!exprOps[op]) throw new Error(`Unknown expression operator: ${op}`);
        const ctx = { db, expr: (e, id, cast) => expression(e, db, id, cast) };
        return `(${exprOps[op](Array.isArray(args) ? args : [args], ctx)})`;
    }

    if (typeof expr === 'string') {
        return expr.includes('.') ? jsonPath(expr, db, castTo) :
            asIdentifier ? validate.column(expr) : escape(expr, db);
    }

    return escape(expr, db);
};

const insertMany = (table, docs, db = 'sqlite', options = {}) => {
    validate.array(docs, 'insertMany');
    if (docs.length === 0) throw new Error('insertMany requires at least one document');

    const allKeys = [...new Set(docs.flatMap(doc => Object.keys(doc)))];
    if (allKeys.length === 0) throw new Error('Documents must have at least one field');

    const columns = allKeys.map(validate.column).join(', ');
    const rows = docs.map(doc =>
        `(${allKeys.map(key => doc.hasOwnProperty(key) ? escape(doc[key], db) : 'NULL').join(', ')})`
    ).join(', ');

    let sql = `INSERT INTO ${validate.column(table)} (${columns}) VALUES ${rows}`;

    if (db === 'pg' && options.returning) {
        const ret = Array.isArray(options.returning) ? options.returning.map(validate.column).join(', ') : '*';
        sql += ` RETURNING ${ret}`;
    }

    return sql;
};

const updateMany = (table, query, update, db = 'sqlite', options = {}) => {
    if (!isObject(update)) throw new Error('Update must be an object');

    const setClauses = [];

    Object.entries(update).forEach(([op, fields]) => {
        if (is$(op)) {
            if (!updateOps[op]) throw new Error(`Unknown update operator: ${op}`);
            setClauses.push(...updateOps[op](fields, db));
        } else {
            setClauses.push(`${validate.column(op)} = ${escape(fields, db)}`);
        }
    });

    if (setClauses.length === 0) throw new Error('Update requires at least one operation');

    let sql = `UPDATE ${validate.column(table)} SET ${setClauses.join(', ')}`;

    if (query && Object.keys(query).length > 0) {
        sql += ` WHERE ${filter(query, db)}`;
    }

    if (db === 'pg' && options.returning) {
        const ret = Array.isArray(options.returning) ? options.returning.map(validate.column).join(', ') : '*';
        sql += ` RETURNING ${ret}`;
    }

    return sql;
};

const deleteMany = (table, query, db = 'sqlite', options = {}) => {
    let sql = `DELETE FROM ${validate.column(table)}`;

    if (query && Object.keys(query).length > 0) {
        sql += ` WHERE ${filter(query, db)}`;
    } else if (!options.allowDeleteAll) {
        throw new Error('deleteMany requires a filter or allowDeleteAll option');
    }

    if (db === 'pg' && options.returning) {
        const ret = Array.isArray(options.returning) ? options.returning.map(validate.column).join(', ') : '*';
        sql += ` RETURNING ${ret}`;
    }

    return sql;
};

class FindQuery {
    constructor(table, query, projection, db) {
        this.table = table;
        this.query = query;
        this.db = db;
        this._fields = null;
        this._sort = '';
        this._skip = '';
        this._limit = '';
        this._distinct = false;

        if (projection) this.select(projection);
    }

    select(proj) {
        if (isObject(proj)) {
            const inc = Object.entries(proj)
                .filter(([_, v]) => v === 1 || v === true)
                .map(([k]) => k.includes('.') ? jsonPath(k, this.db) : validate.column(k));
            this._fields = inc.length > 0 ? inc.join(', ') : null;
        } else if (Array.isArray(proj)) {
            this._fields = proj.map(f => f.includes('.') ? jsonPath(f, this.db) : validate.column(f)).join(', ');
        } else if (typeof proj === 'string') {
            this._fields = proj;
        }
        return this;
    }

    sort(obj) {
        const clauses = Object.entries(obj).map(([k, order]) => {
            const field = k.includes('.') ? jsonPath(k, this.db) : validate.column(k);
            return `${field} ${order === 1 || order === 'asc' ? 'ASC' : 'DESC'}`;
        }).join(', ');
        this._sort = ` ORDER BY ${clauses}`;
        return this;
    }

    skip(n) {
        this._skip = ` OFFSET ${validate.int(n, 'skip')}`;
        return this;
    }

    limit(n) {
        this._limit = ` LIMIT ${validate.int(n, 'limit')}`;
        return this;
    }

    distinct() {
        this._distinct = true;
        return this;
    }

    count() {
        this._fields = 'COUNT(*) AS count';
        this._sort = this._limit = this._skip = '';
        return this;
    }

    toSQL() {
        const fields = this._fields || '*';
        const distinct = this._distinct ? 'DISTINCT ' : '';
        let sql = `SELECT ${distinct}${fields} FROM ${this.table}`;

        if (this.query && Object.keys(this.query).length > 0) {
            sql += ` WHERE ${filter(this.query, this.db)}`;
        }

        return (sql + this._sort + this._limit + this._skip).trim();
    }

    toString() {
        return this.toSQL();
    }
}

const collection = (name, db = 'sqlite') => {
    const table = validate.column(name);

    return {
        find: (query = {}, projection) => new FindQuery(table, query, projection, db),
        findOne: (query = {}, projection) => new FindQuery(table, query, projection, db).limit(1),

        insertOne: (doc, opts = {}) => {
            if (!isObject(doc)) throw new Error('insertOne requires a document object');
            return insertMany(table, [doc], db, opts);
        },
        insertMany: (docs, opts = {}) => insertMany(table, validate.array(docs, 'insertMany'), db, opts),

        updateOne: (query, update, opts = {}) => {
            let sql = updateMany(table, query, update, db, opts);
            if (db !== 'pg') sql += ' LIMIT 1';
            return sql;
        },
        updateMany: (query, update, opts = {}) => updateMany(table, query, update, db, opts),

        deleteOne: (query, opts = {}) => {
            let sql = deleteMany(table, query, db, opts);
            if (db !== 'pg') sql += ' LIMIT 1';
            return sql;
        },
        deleteMany: (query = {}, opts = {}) => deleteMany(table, query, db, opts),

        countDocuments: (query = {}) => {
            let sql = `SELECT COUNT(*) AS count FROM ${table}`;
            if (query && Object.keys(query).length > 0) {
                sql += ` WHERE ${filter(query, db)}`;
            }
            return sql;
        },

        distinct: (field, query = {}) => {
            const f = field.includes('.') ? jsonPath(field, db) : validate.column(field);
            let sql = `SELECT DISTINCT ${f} FROM ${table}`;
            if (query && Object.keys(query).length > 0) {
                sql += ` WHERE ${filter(query, db)}`;
            }
            return sql;
        },
    };
};

const extend = {
    filter: (ops) => Object.assign(filterOps, ops),
    expression: (ops) => Object.assign(exprOps, ops),
    update: (ops) => Object.assign(updateOps, ops),
};


/**
 * Standalone query builder (alternative API)
 */
const db = (collectionName, database = 'sqlite') => {
    return collection(collectionName, database);
};


export {
    filter,
    expression,
    insertMany,
    updateMany,
    deleteMany,
    collection,
    FindQuery,
    extend,
    db,
    escape,
    jsonPath,
    validate,
};

export default collection;