// ============================================
// MONGODB-TO-SQL QUERY BUILDER
// Optimized single source - ~14 KB minified, ~5-7 KB gzipped
// ============================================

// Minimal utilities (inline to reduce imports)
const isObject = (obj) => typeof obj === 'object' && obj !== null && !Array.isArray(obj);
const is$ = (str) => typeof str === 'string' && str.startsWith('$');
const PATTERN = { COL: /^[\w.]+$/, W: /[^\w]/g, NUM: /\.(\d+)/g };

// ============================================
// VALIDATORS (~400 bytes)
// ============================================

const Validate = {
    col: (name, db) => {
        if (!name || typeof name !== 'string') throw new Error(`Invalid column: ${name}`);
        const p = name.split('.');
        if (p.some(x => !x || !/^\w+$/.test(x))) throw new Error(`Invalid column: ${name}`);
        return name;
    },
    alias: (str) => String(str).replace(PATTERN.W, '_'),
    arr: (value, op) => { if (!Array.isArray(value)) throw new Error(`${op} requires array`); return value; },
    int: (v, op) => {
        const n = Number(v);
        if (!Number.isInteger(n) || n < 0) throw new Error(`${op} requires non-negative integer`);
        return n;
    },
};

// ============================================
// ESCAPE (~300 bytes)
// ============================================

const escape = (value, db = 'sqlite') => {
    if (value === null) return 'NULL';
    if (value === undefined) throw new Error('Cannot escape undefined');

    const t = typeof value;
    if (t === 'number' || t === 'bigint') {
        if (!isFinite(value)) throw new Error('Cannot escape non-finite number');
        return String(value);
    }

    if (t === 'boolean') return db === 'pg' ? (value ? 'TRUE' : 'FALSE') : (value ? '1' : '0');

    if (value instanceof Date) {
        if (db === 'mysql') {
            const pad = (n) => String(n).padStart(2, '0');
            const yyyy = value.getUTCFullYear();
            const mm = pad(value.getUTCMonth() + 1);
            const dd = pad(value.getUTCDate());
            const hh = pad(value.getUTCHours());
            const mi = pad(value.getUTCMinutes());
            const ss = pad(value.getUTCSeconds());
            return `'${yyyy}-${mm}-${dd} ${hh}:${mi}:${ss}'`;
        }
        const iso = value.toISOString();
        return db === 'pg' ? `'${iso}'::timestamp` : `'${iso}'`;
    }

    if (isObject(value) || Array.isArray(value)) {
        const j = JSON.stringify(value).replace(/'/g, "''");
        return db === 'pg' ? `'${j}'::jsonb` : `'${j}'`;
    }

    return `'${String(value).replace(/'/g, "''")}'`;
};

// ============================================
// JSON PATH UTILITIES (~800 bytes)
// ============================================

const jsonPath = (path, db = 'sqlite', castType) => {
    if (!path) return '';
    const [col, ...r] = path.split('.');
    if (!r.length) return Validate.col(col, db);

    const a = r.join(',');
    const b = `$.${r.join('.').replace(PATTERN.NUM, '[$1]')}`;

    if (db === 'pg') {
        const x = `(${Validate.col(col, db)}::jsonb #>> '{${a}}')`;
        return castType === 'numeric' ? `(${x})::numeric` :
            castType === 'int' ? `(${x})::int` :
                castType === 'boolean' ? `(${x})::boolean` : x;
    }

    if (db === 'mysql') {
        const x = `JSON_UNQUOTE(JSON_EXTRACT(${Validate.col(col, db)}, '${b}'))`;
        return castType === 'numeric' ? `CAST(${x} AS DECIMAL(20,6))` :
            castType === 'int' ? `CAST(${x} AS SIGNED)` : x;
    }

    return `json_extract(${Validate.col(col, db)}, '${b}')`;
};

const jsonUpdate = (path, value, db, operation = 'set') => {
    const [col, ...r] = path.split('.');
    if (!r.length) return null;

    const c = Validate.col(col, db);
    const a = r.join(',');
    const b = `$.${r.join('.')}`;

    const buildVal = (v) => {
        if (v === null) return 'NULL';
        if (typeof v === 'object') {
            const j = JSON.stringify(v).replace(/'/g, "''");
            return db === 'pg' ? `'${j}'::jsonb` :
                db === 'mysql' ? `CAST('${j}' AS JSON)` : `json('${j}')`;
        }
        return db === 'pg' ? `to_jsonb(${escape(v, db)})` : escape(v, db);
    };

    const extract = {
        pg: `COALESCE((${c}::jsonb #>> '{${a}}')::numeric, ${operation === 'mul' ? 1 : 0})`,
        mysql: `COALESCE(JSON_EXTRACT(${c}, '${b}'), ${operation === 'mul' ? 1 : 0})`,
        sqlite: `COALESCE(json_extract(${c}, '${b}'), ${operation === 'mul' ? 1 : 0})`,
    };

    if (operation === 'remove') {
        return db === 'pg' ? `${c} = ${c}::jsonb #- '{${a}}'` :
            db === 'mysql' ? `${c} = JSON_REMOVE(${c}, '${b}')` :
                `${c} = json_remove(${c}, '${b}')`;
    }

    if (operation === 'inc' || operation === 'mul') {
        const s = operation === 'inc' ? '+' : '*';
        const x = `${extract[db] || extract.sqlite} ${s} ${escape(value, db)}`;
        return db === 'pg' ? `${c} = jsonb_set(${c}::jsonb, '{${a}}', to_jsonb(${x}), true)` :
            db === 'mysql' ? `${c} = JSON_SET(${c}, '${b}', ${x})` :
                `${c} = json_set(${c}, '${b}', ${x})`;
    }

    const jv = buildVal(value);
    return db === 'pg' ? `${c} = jsonb_set(COALESCE(${c}::jsonb, '{}'::jsonb), '{${a}}', ${jv}, true)` :
        db === 'mysql' ? `${c} = JSON_SET(COALESCE(${c}, '{}'), '${b}', ${jv})` :
            `${c} = json_set(COALESCE(${c}, '{}'), '${b}', ${jv})`;
};

const batchJSONUpdate = (fields, db) => {
    const byCol = {}, reg = {};

    Object.entries(fields).forEach(([p, v]) => {
        if (p.includes('.')) {
            const [c] = p.split('.');
            if (!byCol[c]) byCol[c] = {};
            byCol[c][p] = v;
        } else {
            reg[p] = v;
        }
    });

    const upd = [];

    Object.entries(reg).forEach(([k, v]) => {
        upd.push(`${Validate.col(k, db)} = ${escape(v, db)}`);
    });

    Object.entries(byCol).forEach(([col, paths]) => {
        const entries = Object.entries(paths);

        if (entries.length === 1) {
            upd.push(jsonUpdate(entries[0][0], entries[0][1], db));
        } else if (db === 'sqlite' || db === 'mysql') {
            const fn = db === 'mysql' ? 'JSON_SET' : 'json_set';
            const pairs = entries.map(([p, v]) => {
                const jp = `$.${p.split('.').slice(1).join('.')}`;
                const val = typeof v === 'object'
                    ? (db === 'mysql' ? `CAST('${JSON.stringify(v)}' AS JSON)` : db === 'sqlite' ? `json('${JSON.stringify(v)}')` : `'${JSON.stringify(v)}'::jsonb`)
                    : escape(v, db);
                return `'${jp}', ${val}`;
            }).join(', ');
            upd.push(`${Validate.col(col, db)} = ${fn}(COALESCE(${Validate.col(col, db)}, '{}'), ${pairs})`);
        } else {
            let x = `COALESCE(${Validate.col(col, db)}::jsonb, '{}'::jsonb)`;
            entries.forEach(([p, v]) => {
                const jp = p.split('.').slice(1).join(',');
                const val = typeof v === 'object'
                    ? `'${JSON.stringify(v)}'::jsonb`
                    : `to_jsonb(${escape(v, db)})`;
                x = `jsonb_set(${x}, '{${jp}}', ${val}, true)`;
            });
            upd.push(`${Validate.col(col, db)} = ${x}`);
        }
    });

    return upd;
};

// ============================================
// FILTER OPERATORS (~1 KB)
// ============================================

const compareOp = (op) => (v, db) => `${op} ${escape(v, db)}`;

export const filterOps = {
    // Basic comparison
    $eq: compareOp('='),
    $ne: compareOp('!='),
    $gt: compareOp('>'),
    $gte: compareOp('>='),
    $lt: compareOp('<'),
    $lte: compareOp('<='),

    // Array
    $in: (v, db) => {
        Validate.arr(v, '$in');
        return v.length === 0 ? '= 1 AND 1 = 0' : `IN (${v.map(x => escape(x, db)).join(', ')})`;
    },
    $nin: (v, db) => {
        Validate.arr(v, '$nin');
        return v.length === 0 ? '= 1 OR 1 = 1' : `NOT IN (${v.map(x => escape(x, db)).join(', ')})`;
    },

    // String
    $like: (v, db) => `LIKE ${escape(v, db)}`,
    $ilike: (v, db, f) => db === 'pg' ? `ILIKE ${escape(v, db)}` : `${f ? '' : ''}LIKE LOWER(${escape(v, db)})`,
    $nilike: (v, db, f) => db === 'pg' ? `NOT ILIKE ${escape(v, db)}` : `${f ? '' : ''}NOT LIKE LOWER(${escape(v, db)})`,
    // $ilike: (v, db, f) => db === 'pg' ? `ILIKE ${escape(v, db)}` : `LOWER(${f}) LIKE LOWER(${escape(v, db)})`,
    $nlike: (v, db) => `NOT LIKE ${escape(v, db)}`,
    // $nilike: (v, db, f) => db === 'pg' ? `NOT ILIKE ${escape(v, db)}` : `LOWER(${f}) NOT LIKE LOWER(${escape(v, db)})`,
    $regex: (v, db, f) => {
        const p = v instanceof RegExp ? v.source : String(v);
        if (p.length > 1000) { throw new Error('Regex pattern too long (max 1000 chars)'); }
        if (db === 'sqlite') {
            let like = p;
            if (p.startsWith('^') && p.endsWith('$')) like = p.slice(1, -1);
            else if (p.startsWith('^')) like = p.slice(1) + '%';
            else if (p.endsWith('$')) like = '%' + p.slice(0, -1);
            else like = '%' + p.replace(/\.\*/g, '') + '%';
            const lhs = f ? f : Validate.col('name', db);
            return `LIKE ${escape(like, db)}`;
        }
        return (db === 'pg' ? '~' : 'REGEXP') + ` ${escape(p, db)}`;
    },

    // Existence
    $exists: (v) => v ? 'IS NOT NULL' : 'IS NULL',

    // Range
    $between: (v, db) => {
        if (!Array.isArray(v) || v.length !== 2) throw new Error('$between requires [min, max]');
        return `BETWEEN ${escape(v[0], db)} AND ${escape(v[1], db)}`;
    },

    // Modulo
    $mod: (v, db) => {
        if (!Array.isArray(v) || v.length !== 2) throw new Error('$mod requires [divisor, remainder]');
        return `% ${v[0]} = ${v[1]}`;
    },
};

// ============================================
// EXPRESSION OPERATORS (~2 KB)
// ============================================

export const exprOps = {
    // Arithmetic
    $add: (a, c) => `(${a.map(x => c.expr(x)).join(' + ')})`,
    $subtract: (a, c) => `(${a.map(x => c.expr(x)).join(' - ')})`,
    $multiply: (a, c) => `(${a.map(x => c.expr(x)).join(' * ')})`,
    $divide: (a, c) => `(${c.expr(a[0])} * 1.0 / NULLIF(${c.expr(a[1])}, 0))`,
    $mod: (a, c) => `(${c.expr(a[0])} % ${c.expr(a[1])})`,
    $abs: (a, c) => `ABS(${c.expr(a[0])})`,
    $ceil: (a, c) => `CEIL(${c.expr(a[0])})`,
    $floor: (a, c) => `FLOOR(${c.expr(a[0])})`,
    $round: (a, c) => {
        const n = c.expr(a[0]);
        const p = a[1] !== undefined ? c.expr(a[1]) : 0;
        return `ROUND(${n}, ${p})`;
    },
    $pow: (a, c) => `POWER(${c.expr(a[0])}, ${c.expr(a[1])})`,
    $sqrt: (a, c) => `SQRT(${c.expr(a[0])})`,

    // String
    $concat: (a, c) => {
        const x = a.map(v => c.expr(v));
        return c.db === 'pg' || c.db === 'mysql' ? `CONCAT(${x.join(', ')})` : `(${x.join(' || ')})`;
    },
    $upper: (a, c) => `UPPER(${c.expr(a[0])})`,
    $lower: (a, c) => `LOWER(${c.expr(a[0])})`,
    $substr: (a, c) => {
        const s = c.expr(a[0]);
        // MongoDB $substr is 0-based; SQL SUBSTRING is 1-based.
        const st = `(${c.expr(a[1])} + 1)`;
        const l = a[2] !== undefined ? c.expr(a[2]) : null;
        return l ? `SUBSTRING(${s}, ${st}, ${l})` : `SUBSTRING(${s}, ${st})`;
    },
    $trim: (a, c) => `TRIM(${c.expr(a[0])})`,
    $ltrim: (a, c) => `LTRIM(${c.expr(a[0])})`,
    $rtrim: (a, c) => `RTRIM(${c.expr(a[0])})`,
    $strLen: (a, c) => `LENGTH(${c.expr(a[0])})`,
    $replace: (a, c) => `REPLACE(${c.expr(a[0])}, ${c.expr(a[1])}, ${c.expr(a[2])})`,

    // Aggregates
    $sum: (a, c) => a[0] === 1 ? 'COUNT(*)' : `SUM(${c.expr(a[0])})`,
    $avg: (a, c) => {
        const x = `AVG(${c.expr(a[0])})`;
        return c.db === 'mysql' ? `CAST(${x} AS DOUBLE)` : x;
    },
    // SQLite has multi-arg MIN/MAX but no LEAST/GREATEST scalars.
    $min: (a, c) => a.length === 1 ? `MIN(${c.expr(a[0])})` : c.db === 'sqlite' ? `MIN(${a.map(x => c.expr(x)).join(', ')})` : `LEAST(${a.map(x => c.expr(x)).join(', ')})`,
    $max: (a, c) => a.length === 1 ? `MAX(${c.expr(a[0])})` : c.db === 'sqlite' ? `MAX(${a.map(x => c.expr(x)).join(', ')})` : `GREATEST(${a.map(x => c.expr(x)).join(', ')})`,
    $count: () => 'COUNT(*)',
    $stdDevPop: (a, c) => `STDDEV_POP(${c.expr(a[0])})`,
    $stdDevSamp: (a, c) => `STDDEV_SAMP(${c.expr(a[0])})`,

    // Comparison
    $eq: (a, c) => `(${c.expr(a[0])} = ${c.expr(a[1])})`,
    $ne: (a, c) => `(${c.expr(a[0])} <> ${c.expr(a[1])})`,
    $gt: (a, c) => `(${c.expr(a[0])} > ${c.expr(a[1])})`,
    $gte: (a, c) => `(${c.expr(a[0])} >= ${c.expr(a[1])})`,
    $lt: (a, c) => `(${c.expr(a[0])} < ${c.expr(a[1])})`,
    $lte: (a, c) => `(${c.expr(a[0])} <= ${c.expr(a[1])})`,
    $cmp: (a, c) => {
        const x = c.expr(a[0]);
        const y = c.expr(a[1]);
        return `CASE WHEN ${x} < ${y} THEN -1 WHEN ${x} > ${y} THEN 1 ELSE 0 END`;
    },

    // Array
    $in: (a, c) => {
        if (a.length !== 2 || !Array.isArray(a[1])) throw new Error('$in requires [value, array]');
        return `(${c.expr(a[0])} IN (${a[1].map(x => c.expr(x)).join(', ')}))`;
    },
    $nin: (a, c) => {
        if (a.length !== 2 || !Array.isArray(a[1])) throw new Error('$nin requires [value, array]');
        return `(${c.expr(a[0])} NOT IN (${a[1].map(x => c.expr(x)).join(', ')}))`;
    },
    $size: (a, c) => {
        const f = c.expr(a[0]);
        return c.db === 'pg' ? `jsonb_array_length(${f}::jsonb)` :
            c.db === 'mysql' ? `JSON_LENGTH(${f})` :
                `json_array_length(${f})`;
    },

    // Logical
    $and: (a, c) => `(${a.map(x => c.expr(x)).join(' AND ')})`,
    $or: (a, c) => `(${a.map(x => c.expr(x)).join(' OR ')})`,
    $not: (a, c) => `(NOT ${c.expr(a[0])})`,

    // Conditional
    $cond: (a, c) => {
        if (a.length !== 3) throw new Error('$cond requires [condition, then, else]');
        return `CASE WHEN ${c.expr(a[0])} THEN ${c.expr(a[1])} ELSE ${c.expr(a[2])} END`;
    },
    $ifNull: (a, c) => `COALESCE(${c.expr(a[0])}, ${c.expr(a[1])})`,
    $switch: (a, c) => {
        const cfg = a[0];
        if (!cfg?.branches || !Array.isArray(cfg.branches)) {
            throw new Error('$switch requires {branches: [...], default: ...}');
        }
        const b = cfg.branches.map(x => {
            if (!x.case || !x.then) throw new Error('Branch needs {case, then}');
            return `WHEN ${c.expr(x.case)} THEN ${c.expr(x.then)}`;
        }).join(' ');
        const def = cfg.default !== undefined ? c.expr(cfg.default) : 'NULL';
        return `CASE ${b} ELSE ${def} END`;
    },
    $exists: (a, c) => {
        if (a.length !== 2) throw new Error('$exists requires [field, boolean]');
        return `(${c.expr(a[0])} ${a[1] ? 'IS NOT NULL' : 'IS NULL'})`;
    },

    // Date/Time
    $year: (a, c) => {
        const d = c.expr(a[0]);
        return c.db === 'pg' ? `EXTRACT(YEAR FROM ${d})` :
            c.db === 'mysql' ? `YEAR(${d})` :
                `CAST(strftime('%Y', ${d}) AS INTEGER)`;
    },
    $month: (a, c) => {
        const d = c.expr(a[0]);
        return c.db === 'pg' ? `EXTRACT(MONTH FROM ${d})` :
            c.db === 'mysql' ? `MONTH(${d})` :
                `CAST(strftime('%m', ${d}) AS INTEGER)`;
    },
    $dayOfMonth: (a, c) => {
        const d = c.expr(a[0]);
        return c.db === 'pg' ? `EXTRACT(DAY FROM ${d})` :
            c.db === 'mysql' ? `DAY(${d})` :
                `CAST(strftime('%d', ${d}) AS INTEGER)`;
    },
    $dayOfWeek: (a, c) => {
        const d = c.expr(a[0]);
        return c.db === 'pg' ? `EXTRACT(DOW FROM ${d}) + 1` :
            c.db === 'mysql' ? `DAYOFWEEK(${d})` :
                `CAST(strftime('%w', ${d}) AS INTEGER) + 1`;
    },
    $hour: (a, c) => {
        const d = c.expr(a[0]);
        return c.db === 'pg' ? `EXTRACT(HOUR FROM ${d})` :
            c.db === 'mysql' ? `HOUR(${d})` :
                `CAST(strftime('%H', ${d}) AS INTEGER)`;
    },
    $minute: (a, c) => {
        const d = c.expr(a[0]);
        return c.db === 'pg' ? `EXTRACT(MINUTE FROM ${d})` :
            c.db === 'mysql' ? `MINUTE(${d})` :
                `CAST(strftime('%M', ${d}) AS INTEGER)`;
    },
    $second: (a, c) => {
        const d = c.expr(a[0]);
        return c.db === 'pg' ? `EXTRACT(SECOND FROM ${d})` :
            c.db === 'mysql' ? `SECOND(${d})` :
                `CAST(strftime('%S', ${d}) AS INTEGER)`;
    },
    $week: (a, c) => {
        const d = c.expr(a[0]);
        return c.db === 'pg' ? `EXTRACT(WEEK FROM ${d})` :
            c.db === 'mysql' ? `WEEK(${d})` :
                `CAST(strftime('%W', ${d}) AS INTEGER)`;
    },

    // Type conversion
    $toString: (a, c) => c.db === 'mysql' ? `CAST(${c.expr(a[0])} AS CHAR)` : `CAST(${c.expr(a[0])} AS TEXT)`,
    $toInt: (a, c) => c.db === 'mysql' ? `CAST(${c.expr(a[0])} AS SIGNED)` : `CAST(${c.expr(a[0])} AS INTEGER)`,
    $toDouble: (a, c) => {
        const x = c.expr(a[0]);
        return c.db === 'pg' ? `CAST(${x} AS DOUBLE PRECISION)` :
            c.db === 'mysql' ? `CAST(${x} AS DECIMAL(20,6))` :
                `CAST(${x} AS REAL)`;
    },
    $toBool: (a, c) => c.db === 'mysql' ? `IF(${c.expr(a[0])}, 1, 0)` : `CAST(${c.expr(a[0])} AS BOOLEAN)`,
    $toDate: (a, c) => c.db === 'sqlite' ? `datetime(${c.expr(a[0])})` : c.db === 'mysql' ? `CAST(${c.expr(a[0])} AS DATETIME)` : `CAST(${c.expr(a[0])} AS TIMESTAMP)`,

    // Literal
    $literal: (a) => escape(a[0]),
};

// ============================================
// UPDATE OPERATORS (~800 bytes)
// ============================================

export const updateOps = {
    $set: (f, db) => batchJSONUpdate(f, db),

    $inc: (f, db) => Object.entries(f).map(([k, v]) =>
        k.includes('.') ? jsonUpdate(k, v, db, 'inc') : `${Validate.col(k, db)} = ${Validate.col(k, db)} + ${escape(v, db)}`
    ),

    $mul: (f, db) => Object.entries(f).map(([k, v]) =>
        k.includes('.') ? jsonUpdate(k, v, db, 'mul') : `${Validate.col(k, db)} = ${Validate.col(k, db)} * ${escape(v, db)}`
    ),

    $min: (f, db) => Object.entries(f).map(([k, v]) => {
        const field = k.includes('.') ? jsonPath(k, db, 'numeric') : Validate.col(k, db);
        const fn = db === 'sqlite' ? 'MIN' : 'LEAST';
        return k.includes('.') ? jsonUpdate(k, v, db) : `${field} = ${fn}(${field}, ${escape(v, db)})`;
    }),

    $max: (f, db) => Object.entries(f).map(([k, v]) => {
        const field = k.includes('.') ? jsonPath(k, db, 'numeric') : Validate.col(k, db);
        const fn = db === 'sqlite' ? 'MAX' : 'GREATEST';
        return k.includes('.') ? jsonUpdate(k, v, db) : `${field} = ${fn}(${field}, ${escape(v, db)})`;
    }),

    $unset: (f, db) => Object.keys(f).map(k =>
        k.includes('.') ? jsonUpdate(k, null, db, 'remove') : `${Validate.col(k, db)} = NULL`
    ),

    $currentDate: (f, db) => {
        const now = db === 'pg' ? 'CURRENT_TIMESTAMP' : db === 'mysql' ? 'NOW()' : "datetime('now')";
        return Object.entries(f).map(([k, spec]) => {
            const val = (isObject(spec) && spec.$type === 'timestamp') ? now : now;
            return k.includes('.') ? jsonUpdate(k, val, db) : `${Validate.col(k, db)} = ${val}`;
        });
    },

    $rename: (f, db) => {
        return Object.entries(f).flatMap(([old, neu]) => {
            if (old.includes('.') || neu.includes('.')) {
                throw new Error('$rename for JSON fields not supported');
            }
            const o = Validate.col(old, db);
            const n = Validate.col(neu, db);
            return [`${n} = ${o}`, `${o} = NULL`];
        });
    },
};

// ============================================
// STAGE HANDLERS (~2 KB)
// ============================================

export const stageHandlers = {
    $match: (a, s, db, h) => {
        const frag = h.filter(a, db);
        if (s.groupBy) {
            const havingFrag = h.replace(frag, s.aggExprs);
            s.having.push(havingFrag);
        } else {
            s.where.push(frag);
        }
    },

    $project: (a, s, db, h) => {
        h.applyWhere();

        const cols = Object.entries(a).map(([k, v]) => {
            const alias = Validate.alias(k);

            if (v === 1 || v === true) {
                const f = k.includes('.') ? jsonPath(k, db) : Validate.col(k, db);
                return `${f} AS ${alias}`;
            } else if (v === 0 || v === false) {
                return null;
            } else if (isObject(v) || is$(v)) {
                const x = h.expr(v, db);
                s.aggExprs[alias] = x;
                return `${x} AS ${alias}`;
            } else {
                return `${escape(v, db)} AS ${alias}`;
            }
        }).filter(Boolean);

        s.sql = `SELECT ${cols.join(', ')} FROM ${h.wrap(s.sql)}`;
    },

    $addFields: (a, s, db, h) => {
        h.applyWhere();

        const existing = s.groupBy ? Object.keys(s.aggExprs).map(k => `${k} AS ${k}`) : ['*'];
        const newFields = Object.entries(a).map(([k, v]) => {
            const x = h.expr(v, db);
            const alias = Validate.alias(k);
            s.aggExprs[alias] = x;
            return `${x} AS ${alias}`;
        });

        const all = existing[0] === '*' ? ['*', ...newFields] : [...existing, ...newFields];
        s.sql = `SELECT ${all.join(', ')} FROM ${h.wrap(s.sql)}`;
    },

    $set: (a, s, db, h) => stageHandlers.$addFields(a, s, db, h),

    $group: (a, s, db, h) => {
        h.applyWhere();

        let id = null, grp = null;

        if (a._id === null) {
            id = null;
            grp = null;
        } else if (isObject(a._id) && !is$(a._id)) {
            const parts = Object.entries(a._id).map(([k, v]) => {
                const x = h.expr(v, db);
                return `${x} AS ${Validate.alias(k)}`;
            });
            id = parts.join(', ');
            grp = Object.values(a._id).map(v => h.expr(v, db)).join(', ');
        } else {
            id = `${h.expr(a._id, db)} AS _id`;
            grp = h.expr(a._id, db);
        }

        s.aggExprs = {};
        const aggs = Object.entries(a)
            .filter(([k]) => k !== '_id')
            .map(([k, v]) => {
                const x = h.expr(v, db);
                const alias = Validate.alias(k);
                s.aggExprs[alias] = x;
                return `${x} AS ${alias}`;
            });

        const parts = [];
        if (id) parts.push(id);
        parts.push(...aggs);

        s.sql = `SELECT ${parts.join(', ')} FROM ${h.wrap(s.sql)}`;
        s.groupBy = grp;
    },

    $sort: (a, s, db) => {
        const clauses = Object.entries(a).map(([k, ord]) => {
            // When the field was produced by an earlier $project/$addFields/$group
            // stage, sort by its output alias: the raw expression may reference
            // columns that the wrapping sub-select no longer exposes.
            const f = s.aggExprs[k] !== undefined
                ? Validate.alias(k)
                : (k.includes('.') ? jsonPath(k, db) : Validate.col(k, db));
            return `${f} ${ord === 1 || ord === 'asc' ? 'ASC' : 'DESC'}`;
        }).join(', ');
        s.order = ` ORDER BY ${clauses}`;
    },

    $limit: (a, s) => s.limit = ` LIMIT ${Validate.int(a, '$limit')}`,

    $skip: (a, s) => s.offset = ` OFFSET ${Validate.int(a, '$skip')}`,

    $count: (a, s, db, h) => {
        h.applyWhere();
        s.sql = `SELECT COUNT(*) AS ${Validate.alias(a)} FROM ${h.wrap(s.sql)}`;
        s.order = s.limit = s.offset = '';
        s.aggExprs = {};
    },

    $sample: (a, s, db, h) => {
        h.applyWhere();

        const size = Validate.int(a.size, '$sample');
        const randFn = db === 'pg' ? 'RANDOM()' : db === 'mysql' ? 'RAND()' : 'RANDOM()';

        s.sql = `SELECT * FROM ${h.wrap(s.sql)} ORDER BY ${randFn} LIMIT ${size}`;
        s.order = '';
        s.limit = '';
        s.offset = '';
    },

    $sortByCount: (a, s, db, h) => {
        h.applyWhere();
        const x = h.expr(a, db);

        s.sql = `SELECT ${x} AS _id, COUNT(*) AS count FROM ${h.wrap(s.sql)}`;
        s.groupBy = x;
        s.order = ' ORDER BY count DESC';
        s.aggExprs = { count: 'COUNT(*)' };
    },

    $bucket: (a, s, db, h) => {
        const { groupBy, boundaries, default: def, output } = a;

        if (!boundaries || !Array.isArray(boundaries)) {
            throw new Error('$bucket requires boundaries array');
        }

        h.applyWhere();

        const x = h.expr(groupBy, db);

        let caseExpr = 'CASE ';
        for (let i = 0; i < boundaries.length - 1; i++) {
            caseExpr += `WHEN ${x} >= ${boundaries[i]} AND ${x} < ${boundaries[i + 1]} THEN ${boundaries[i]} `;
        }
        if (def) caseExpr += `ELSE ${escape(def, db)} `;
        caseExpr += 'END';

        const outFields = output ? Object.entries(output).map(([k, v]) => {
            return `${h.expr(v, db)} AS ${Validate.alias(k)}`;
        }).join(', ') : '';

        s.sql = `SELECT ${caseExpr} AS _id, COUNT(*) AS count${outFields ? ', ' + outFields : ''} FROM ${h.wrap(s.sql)}`;
        s.groupBy = caseExpr;
    },
};

// ============================================
// QUERY BUILDER FACTORY (~4 KB)
// ============================================

export const createQueryBuilder = (config = {}) => {
    const fOps = config.filterOps || {};
    const eOps = config.exprOps || {};
    const uOps = config.updateOps || {};
    const sOps = config.stageHandlers || {};
    const debug = config.debug || false;

    const logQuery = (sql, params = {}) => {
        if (debug) {
            console.log('[SQL Query]', sql);
            console.log('[Parameters]', JSON.stringify(params, null, 1));
        }
    };

    // FILTER
    const filter = (q, db = 'sqlite') => {
        if (!isObject(q)) return q;

        return Object.entries(q).map(([k, v]) => {
            if (is$(k)) {
                if (k === '$and' || k === '$or') {
                    Validate.arr(v, k);
                    const op = k === '$and' ? 'AND' : 'OR';
                    return `(${v.map(x => filter(x, db)).join(` ${op} `)})`;
                }
                if (k === '$nor') {
                    Validate.arr(v, k);
                    return `NOT (${v.map(x => filter(x, db)).join(' OR ')})`;
                }
                if (k === '$not') return `NOT (${filter(v, db)})`;
                if (k === '$expr') return expr(v, db);
                throw new Error(`Unknown filter operator: ${k}`);
            }

            const isJson = k.includes('.');
            const f = isJson ? jsonPath(k, db) : Validate.col(k, db);

            if (isObject(v)) {
                const conds = Object.entries(v).map(([op, val]) => {
                    const castType = typeof val === 'number' ? 'numeric'
                        : typeof val === 'boolean' ? 'boolean' : undefined;
                    const f2 = isJson && castType ? jsonPath(k, db, castType) : f;
                    if (op === '$not' && isObject(val)) {
                        const inner = Object.entries(val).map(([op2, val2]) => {
                            if (!fOps[op2]) throw new Error(`Unknown operator: ${op2}`);
                            return `${f2} ${fOps[op2](val2, db, f2)}`;
                        }).join(' AND ');
                        return `NOT (${inner})`;
                    }
                    if (!fOps[op]) throw new Error(`Unknown operator: ${op}`);
                    if (op === '$ilike' || op === '$nilike') {
                        return db === 'pg' ? `${f} ${fOps[op](val, db)}` : `LOWER(${f}) ${fOps[op](val, db)}`;
                    }
                    return `${f2} ${fOps[op](val, db, f2)}`;
                });
                return conds.length > 1 ? `(${conds.join(' AND ')})` : conds[0];
            }

            const castType = typeof v === 'number' ? 'numeric' : typeof v === 'boolean' ? 'boolean' : undefined;
            const f2 = isJson && castType ? jsonPath(k, db, castType) : f;
            return `${f2} = ${escape(v, db)}`;
        }).join(' AND ');
    };

    // EXPRESSION
    const expr = (x, db = 'sqlite', id = false, cast = 'text') => {
        if (is$(x)) {
            const v = x.slice(1);
            return v.includes('.') ? jsonPath(v, db, cast) : Validate.col(v, db);
        }

        if (isObject(x)) {
            const [op, args] = Object.entries(x)[0];
            if (!eOps[op]) throw new Error(`Unknown expression operator: ${op}`);
            const ctx = { db, expr: (e, i, c) => expr(e, db, i, c) };
            return eOps[op](Array.isArray(args) ? args : [args], ctx);
        }

        if (typeof x === 'string') {
            return x.includes('.') ? jsonPath(x, db, cast) :
                id ? Validate.col(x, db) : escape(x, db);
        }

        return escape(x, db);
    };

    // AGGREGATE
    const aggregate = (pipeline) => (table, db = 'sqlite') => {
        Validate.arr(pipeline, 'aggregate');

        let s = {
            sql: `SELECT * FROM ${Validate.col(table, db)}`,
            where: [],
            having: [],
            groupBy: null,
            aggExprs: {},
            order: '',
            limit: '',
            offset: '',
            counter: 0,
        };

        const wrap = (sql) => `(${sql}) AS t${++s.counter}`;

        const applyWhere = () => {
            if (s.where.length) {
                s.sql += ` WHERE ${s.where.join(' AND ')}`;
                s.where = [];
            }
        };

        const replace = (str, map) => {
            let r = str;
            Object.entries(map)
                .sort((a, b) => b[0].length - a[0].length)
                .forEach(([k, v]) => {
                    r = r.replace(new RegExp(`\\b${k}\\b`, 'g'), v);
                });
            return r;
        };

        const h = { wrap, applyWhere, replace, filter, expr };

        pipeline.forEach((stage, i) => {
            if (!isObject(stage)) throw new Error(`Stage ${i} must be an object`);
            const [op, args] = Object.entries(stage)[0];
            if (!sOps[op]) throw new Error(`Unknown pipeline operator: ${op}`);
            sOps[op](args, s, db, h);
        });

        let sql = s.sql;
        if (s.where.length) sql += ` WHERE ${s.where.join(' AND ')}`;
        if (s.groupBy) sql += ` GROUP BY ${s.groupBy}`;
        if (s.having.length) sql += ` HAVING ${s.having.join(' AND ')}`;
        sql += `${s.order}${s.limit}${s.offset}`;

        logQuery(sql, { table, db, pipeline });

        return sql.trim();
    };

    // CRUD OPERATIONS
    const insertMany = (table, docs, db = 'sqlite', options = {}) => {
        Validate.arr(docs, 'insertMany');
        if (docs.length === 0) throw new Error('insertMany requires at least one document');

        const allKeys = [...new Set(docs.flatMap(d => Object.keys(d)))];
        if (allKeys.length === 0) throw new Error('Documents must have at least one field');

        // MySQL lexes type-keyword names (e.g. `longText` → LONGTEXT) as keywords
        // even inside an INSERT column list, so quote mysql identifiers here.
        const colName = (name) => db === 'mysql' ? `\`${name}\`` : name;

        const columns = allKeys.map((k) => colName(Validate.col(k, db))).join(', ');
        const rows = docs.map(doc =>
            `(${allKeys.map(k => doc.hasOwnProperty(k) ? escape(doc[k], db) : 'NULL').join(', ')})`
        ).join(', ');

        let sql = `INSERT INTO ${colName(Validate.col(table, db))} (${columns}) VALUES ${rows}`;

        if (db === 'pg' && options.returning) {
            const ret = Array.isArray(options.returning) ? options.returning.map(Validate.col).join(', ') : '*';
            sql += ` RETURNING ${ret}`;
        }

        logQuery(sql, { table, db, docs, options });

        return sql;
    };

    const updateMany = (table, query, update, db = 'sqlite', options = {}) => {
        if (!isObject(update)) throw new Error('Update must be an object');

        const setClauses = [];

        Object.entries(update).forEach(([op, fields]) => {
            if (is$(op)) {
                if (!uOps[op]) throw new Error(`Unknown update operator: ${op}`);
                const result = uOps[op](fields, db);
                setClauses.push(...(Array.isArray(result) ? result : [result]));
            } else {
                const k = op.includes('.') ? op : Validate.col(op, db);
                setClauses.push(op.includes('.')
                    ? jsonUpdate(op, fields, db)
                    : `${k} = ${escape(fields, db)}`
                );
            }
        });

        if (setClauses.length === 0) throw new Error('Update requires at least one operation');

        let sql = `UPDATE ${Validate.col(table, db)} SET ${setClauses.join(', ')}`;

        if (query && Object.keys(query).length > 0) {
            sql += ` WHERE ${filter(query, db)}`;
        }

        if (db === 'pg' && options.returning) {
            const ret = Array.isArray(options.returning) ? options.returning.map(Validate.col).join(', ') : '*';
            sql += ` RETURNING ${ret}`;
        }

        logQuery(sql, { table, db, options });

        return sql;
    };

    const deleteMany = (table, query, db = 'sqlite', options = {}) => {
        let sql = `DELETE FROM ${Validate.col(table, db)}`;

        if (query && Object.keys(query).length > 0) {
            sql += ` WHERE ${filter(query, db)}`;
        } else if (!options.allowDeleteAll) {
            throw new Error('deleteMany requires a filter or allowDeleteAll option');
        }

        if (db === 'pg' && options.returning) {
            const ret = Array.isArray(options.returning) ? options.returning.map(Validate.col).join(', ') : '*';
            sql += ` RETURNING ${ret}`;
        }

        logQuery(sql, { table, db, options });

        return sql;
    };

    // FIND QUERY CLASS
    class FindQuery {
        constructor(table, query, projection, db) {
            this.table = table;
            this.query = query;
            this.db = db;
            this._fields = null;
            this._sort = '';
            this._sortObj = null;
            this._skip = '';
            this._limit = '';
            this._distinct = false;

            if (projection) this.select(projection);
        }

        select(proj) {
            if (isObject(proj)) {
                const inc = Object.entries(proj)
                    .filter(([_, v]) => v === 1 || v === true)
                    .map(([k]) => k.includes('.') ? jsonPath(k, this.db) : Validate.col(k, db));

                const comp = Object.entries(proj)
                    .filter(([_, v]) => isObject(v) || is$(v))
                    .map(([k, v]) => `${expr(v, this.db)} AS ${Validate.alias(k)}`);

                const all = [...inc, ...comp];
                this._fields = all.length > 0 ? all.join(', ') : null;
            } else if (Array.isArray(proj)) {
                this._fields = proj.map(f => f.includes('.') ? jsonPath(f, this.db) : Validate.col(f, db)).join(', ');
            } else if (typeof proj === 'string') {
                this._fields = proj;
            }
            return this;
        }

        sort(obj) { this._sortObj = obj; return this; }

        skip(n) {
            this._skip = ` OFFSET ${Validate.int(n, 'skip')}`;
            return this;
        }

        limit(n) {
            this._limit = ` LIMIT ${Validate.int(n, 'limit')}`;
            return this;
        }

        distinct() {
            this._distinct = true;
            return this;
        }

        count() {
            this._fields = 'COUNT(*) AS count';
            this._sort = this._limit = this._skip = '';
            this._distinct = false;
            return this;
        }

        toSQL() {
            const fields = this._fields || '*';
            const distinct = this._distinct ? 'DISTINCT ' : '';
            let sql = `SELECT ${distinct}${fields} FROM ${this.table}`;

            if (this.query && Object.keys(this.query).length > 0) {
                sql += ` WHERE ${filter(this.query, this.db)}`;
            }

            if (this._sortObj) {
                const clauses = Object.entries(this._sortObj).map(([k, ord]) => {
                    const f = k.includes('.') ? jsonPath(k, this.db) : Validate.col(k, db);
                    const asc = ord === 1 || ord === 'asc';
                    if (this.db === 'pg') return `${f} ${asc ? 'ASC NULLS FIRST' : 'DESC NULLS LAST'}`;
                    if (this.db === 'mysql' && this._distinct) return `${f} ${asc ? 'ASC' : 'DESC'}`;
                    const nullsFirst = asc ? `CASE WHEN ${f} IS NULL THEN 0 ELSE 1 END` : `CASE WHEN ${f} IS NULL THEN 1 ELSE 0 END`;
                    return `${nullsFirst}, ${f} ${asc ? 'ASC' : 'DESC'}`;
                }).join(', ');
                this._sort = ` ORDER BY ${clauses}`;
            } else {
                this._sort = '';
            }
            logQuery(sql + this._sort + this._limit + this._skip, { table: this.table, db: this.db, query: this.query });
            return (sql + this._sort + this._limit + this._skip).trim();
        }

        toString() {
            return this.toSQL();
        }
    }

    // COLLECTION API
    const collection = (name, db = 'sqlite') => {
        const table = Validate.col(name, db);

        return {
            find: (query = {}, projection) => new FindQuery(table, query, projection, db),
            findOne: (query = {}, projection) => new FindQuery(table, query, projection, db).limit(1),

            insertOne: (doc, opts = {}) => {
                if (!isObject(doc)) throw new Error('insertOne requires a document object');
                return insertMany(table, [doc], db, opts);
            },
            insertMany: (docs, opts = {}) => insertMany(table, Validate.arr(docs, 'insertMany'), db, opts),

            updateOne: (query, update, opts = {}) => {
                let sql = updateMany(table, query, update, db, opts);
                if (db !== 'pg') sql += ' LIMIT 1';
                logQuery(sql, { table, db, query, update, opts });
                return sql;
            },
            updateMany: (query, update, opts = {}) => updateMany(table, query, update, db, opts),

            deleteOne: (query, opts = {}) => {
                let sql = deleteMany(table, query, db, opts);
                if (db !== 'pg') sql += ' LIMIT 1';
                logQuery(sql, { table, db, query, opts });
                return sql;
            },
            deleteMany: (query = {}, opts = {}) => deleteMany(table, query, db, opts),

            countDocuments: (query = {}) => {
                let sql = `SELECT COUNT(*) AS count FROM ${table}`;
                if (query && Object.keys(query).length > 0) {
                    sql += ` WHERE ${filter(query, db)}`;
                }

                logQuery(sql, { table, db, query });
                return sql;
            },

            distinct: (field, query = {}) => {
                const f = field.includes('.') ? jsonPath(field, db) : Validate.col(field, db);
                let sql = `SELECT DISTINCT ${f} FROM ${table}`;
                if (query && Object.keys(query).length > 0) {
                    sql += ` WHERE ${filter(query, db)}`;
                }
                logQuery(sql, { table, db, query });
                return sql;
            },

            aggregate: (pipeline) => aggregate(pipeline)(table, db),
        };
    };

    // EXTEND API
    const extend = {
        filter: (ops) => Object.assign(fOps, ops),
        expression: (ops) => Object.assign(eOps, ops),
        update: (ops) => Object.assign(uOps, ops),
        stage: (ops) => Object.assign(sOps, ops),
    };

    const db = (collectionName, database = 'sqlite') => collection(collectionName, database);


    return {
        filter,
        expression: expr,
        aggregate,
        insertMany,
        updateMany,
        deleteMany,
        collection,
        FindQuery,
        extend,
        db
    };
};

// ============================================
// DEFAULT EXPORTS (Full Version)
// ============================================

const fullBuilder = createQueryBuilder();

export const { filter, expression, aggregate, insertMany, updateMany, deleteMany, collection, FindQuery, extend, db } = fullBuilder;
export default collection;

// Re-export utilities for building custom versions
export { escape as escape, jsonPath as jsonPath, Validate as validate };
export { isObject as isObject, is$ as is$ };
 
