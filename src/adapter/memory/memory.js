// ============================================
// IN-MEMORY MONGODB ADAPTER
// Optimized single source ~15 KB minified, ~6-8 KB gzipped
// ============================================

// Minimal utilities
const isObject = (obj) => typeof obj === 'object' && obj !== null && !Array.isArray(obj);
const is$ = (str) => typeof str === 'string' && str.startsWith('$');

// ============================================
// EQUALITY & NESTED ACCESS (~600 bytes)
// ============================================

const deepEquals = (a, b) => {
    if (a === b) return true;
    if (!a || !b || a.constructor !== b.constructor) return a !== a && b !== b;
    if (a.constructor === Array) return a.length === b.length && a.every((v, i) => deepEquals(v, b[i]));
    if (a.constructor === Object) {
        const k = Object.keys(a);
        return k.length === Object.keys(b).length && k.every(key => key in b && deepEquals(a[key], b[key]));
    }
    return false;
};

const getPath = (obj, key) => {
    const keys = Array.isArray(key) ? key : key.split('.');
    let curr = obj;
    for (let i = 0; i < keys.length; i++) {
        if (curr == null) return undefined;
        curr = curr[keys[i]];
    }
    return curr;
};

const setPath = (obj, path, val) => {
    const keys = Array.isArray(path) ? path : path.split('.');
    let curr = obj;
    for (let i = 0; i < keys.length - 1; i++) {
        const k = keys[i];
        const next = keys[i + 1];
        if (!(k in curr) || typeof curr[k] !== 'object' || curr[k] === null) {
            curr[k] = /^\d+$/.test(next) ? [] : {};
        }
        curr = curr[k];
    }
    curr[keys[keys.length - 1]] = val;
};

const delPath = (obj, path) => {
    const keys = Array.isArray(path) ? path : path.split('.');
    let curr = obj;
    for (let i = 0; i < keys.length - 1; i++) {
        if (!(keys[i] in curr)) return;
        curr = curr[keys[i]];
    }
    delete curr[keys[keys.length - 1]];
};

const clone = (obj) => {
    if (obj instanceof Date) return new Date(obj.getTime());
    if (Array.isArray(obj)) return obj.map(clone);
    if (isObject(obj)) {
        const out = {};
        for (const k in obj) out[k] = clone(obj[k]);
        return out;
    }
    return obj;
};

// ============================================
// FILTER OPERATORS (~1.5 KB)
// ============================================

export const filterOps = {
    $eq: (q, v) => deepEquals(q, v),
    $ne: (q, v) => !deepEquals(q, v),
    $gt: (q, v) => v > q,
    $gte: (q, v) => v >= q,
    $lt: (q, v) => v < q,
    $lte: (q, v) => v <= q,
    
    $in: (q, v) => (Array.isArray(q) ? q : [q]).some(x => deepEquals(x, v)),
    $nin: (q, v) => !(Array.isArray(q) ? q : [q]).some(x => deepEquals(x, v)),
    
    $and: (q, v, filter) => q.every(c => filter(c)(v)),
    $or: (q, v, filter) => q.some(c => filter(c)(v)),
    $not: (q, v, filter) => !filter(q)(v),
    $nor: (q, v, filter) => !q.some(c => filter(c)(v)),
    
    $regex: (q, v) => (q instanceof RegExp ? q : new RegExp(q)).test(String(v)),
    $exists: (q, v) => q ? (v !== undefined && v !== null) : (v === undefined || v === null),
    
    $type: (q, v) => ({
        string: typeof v === 'string',
        number: typeof v === 'number',
        boolean: typeof v === 'boolean',
        array: Array.isArray(v),
        object: isObject(v),
        null: v === null,
        undefined: v === undefined,
        date: v instanceof Date,
    }[q] || false),
    
    $mod: (q, v) => typeof v === 'number' && (v % q[0]) === q[1],
    $elemMatch: (q, v, filter) => Array.isArray(v) && v.some(i => filter(q)(i)),
    $all: (q, v) => Array.isArray(v) && q.every(x => v.some(i => deepEquals(i, x))),
    $size: (q, v) => Array.isArray(v) && v.length === q,
    $where: (q, v) => q.call(v),
    $expr: (q, v, filter, expression) => expression(q)(v),
    $like: (q, v) => new RegExp(`^${String(q).replace(/%/g, '.*').replace(/_/g, '.')}$`).test(String(v)),
    $ilike: (q, v) => new RegExp(`^${String(q).replace(/%/g, '.*').replace(/_/g, '.')}$`, 'i').test(String(v)),
    $nlike: (q, v) => !new RegExp(`^${String(q).replace(/%/g, '.*').replace(/_/g, '.')}$`).test(String(v)),
    $nilike: (q, v) => !new RegExp(`^${String(q).replace(/%/g, '.*').replace(/_/g, '.')}$`, 'i').test(String(v)),
    $between: (q, v) => Array.isArray(q) && q.length === 2 && v >= q[0] && v <= q[1],
};

// ============================================
// EXPRESSION OPERATORS (~2.5 KB)
// ============================================

const asDate = (v) => {
    if (v instanceof Date) return isNaN(v) ? null : v;
    if (typeof v === 'number' && isFinite(v)) return new Date(v);
    if (typeof v === 'string' && v) {
        const d = new Date(v);
        return isNaN(d) ? null : d;
    }
    return null;
};

const part = (d, get) => (d ? get(d) : null);

export const exprOps = {
    // Arithmetic
    $add: (a, c) => a.map(x => c.expr(x) || 0).reduce((s, n) => s + n, 0),
    $subtract: (a, c) => a.map(x => c.expr(x) || 0).reduce((s, n) => s - n),
    $multiply: (a, c) => a.map(x => c.expr(x) || 0).reduce((s, n) => s * n, 1),
    $divide: ([x, y], c) => {
        const [a, b] = [c.expr(x) || 0, c.expr(y) || 0];
        return b === 0 ? null : a / b;
    },
    $mod: ([x, y], c) => {
        const [a, b] = [c.expr(x), c.expr(y)];
        return b === 0 ? null : a % b;
    },
    $abs: (a, c) => Math.abs(c.expr(a[0])),
    $ceil: (a, c) => Math.ceil(c.expr(a[0])),
    $floor: (a, c) => Math.floor(c.expr(a[0])),
    $round: (a, c) => {
        const n = c.expr(a[0]);
        const p = a[1] !== undefined ? c.expr(a[1]) : 0;
        return Math.round(n * Math.pow(10, p)) / Math.pow(10, p);
    },
    $pow: (a, c) => Math.pow(c.expr(a[0]), c.expr(a[1])),
    $sqrt: (a, c) => Math.sqrt(c.expr(a[0])),
    
    // String
    $concat: (a, c) => a.map(x => String(c.expr(x) || '')).join(''),
    $upper: (a, c) => String(c.expr(a[0]) || '').toUpperCase(),
    $lower: (a, c) => String(c.expr(a[0]) || '').toLowerCase(),
    $substr: (a, c) => {
        const s = String(c.expr(a[0]) || '');
        const st = c.expr(a[1]);
        const l = a[2] !== undefined ? c.expr(a[2]) : s.length;
        return s.substr(st, l);
    },
    $strLen: (a, c) => String(c.expr(a[0]) || '').length,
    $trim: (a, c) => String(c.expr(a[0]) || '').trim(),
    $ltrim: (a, c) => String(c.expr(a[0]) || '').trimStart(),
    $rtrim: (a, c) => String(c.expr(a[0]) || '').trimEnd(),
    $replace: (a, c) => {
        const [s, srch, rep] = a.map(x => String(c.expr(x)));
        return s.replace(new RegExp(srch, 'g'), rep);
    },
    $split: (a, c) => String(c.expr(a[0]) || '').split(String(c.expr(a[1]))),
    
    // Aggregates
    $min: (a, c) => Math.min(...a.map(x => c.expr(x))),
    $max: (a, c) => Math.max(...a.map(x => c.expr(x))),
    $avg: (a, c) => {
        const v = a.map(x => c.expr(x) || 0);
        return v.reduce((s, n) => s + n, 0) / v.length;
    },
    $sum: (a, c) => a.map(x => c.expr(x) || 0).reduce((s, n) => s + n, 0),
    
    // Comparison
    // Three-way comparison; SQL emits `CASE WHEN x < y THEN -1 WHEN x > y THEN 1
    // ELSE 0 END`, so null sorts first like it does in the databases.
    $cmp: ([a, b], c) => {
        const x = c.expr(a);
        const y = c.expr(b);
        if (deepEquals(x, y)) return 0;
        if (x === null || x === undefined) return -1;
        if (y === null || y === undefined) return 1;
        return x < y ? -1 : 1;
    },
    $eq: ([a, b], c) => deepEquals(c.expr(a), c.expr(b)),
    $ne: ([a, b], c) => !deepEquals(c.expr(a), c.expr(b)),
    $gt: ([a, b], c) => c.expr(a) > c.expr(b),
    $gte: ([a, b], c) => c.expr(a) >= c.expr(b),
    $lt: ([a, b], c) => c.expr(a) < c.expr(b),
    $lte: ([a, b], c) => c.expr(a) <= c.expr(b),
    
    // Array
    $in: ([v, a], c) => {
        const arr = c.expr(a);
        const val = c.expr(v);
        return Array.isArray(arr) && arr.some(i => deepEquals(i, val));
    },
    $nin: ([v, a], c) => {
        const arr = c.expr(a);
        const val = c.expr(v);
        return !Array.isArray(arr) || !arr.some(i => deepEquals(i, val));
    },
    $size: (a, c) => {
        const arr = c.expr(a[0]);
        return Array.isArray(arr) ? arr.length : null;
    },
    $arrayElemAt: ([a, i], c) => {
        const arr = c.expr(a);
        const idx = c.expr(i);
        if (!Array.isArray(arr)) return null;
        return idx >= 0 ? arr[idx] : arr[arr.length + idx];
    },
    $slice: ([a, n, l], c) => {
        const arr = c.expr(a);
        if (!Array.isArray(arr)) return null;
        const num = c.expr(n);
        if (l !== undefined) {
            const lim = c.expr(l);
            return arr.slice(num, num + lim);
        }
        return num >= 0 ? arr.slice(0, num) : arr.slice(num);
    },
    $map: ([a, v, e], c) => {
        const arr = c.expr(a);
        if (!Array.isArray(arr)) return null;
        return arr.map(i => c.expr(e, { ...c.ctx, [v]: i }));
    },
    $filter: ([a, v, e], c) => {
        const arr = c.expr(a);
        if (!Array.isArray(arr)) return null;
        return arr.filter(i => c.expr(e, { ...c.ctx, [v]: i }));
    },
    $reduce: ([a, i, e], c) => {
        const arr = c.expr(a);
        if (!Array.isArray(arr)) return c.expr(i);
        return arr.reduce((acc, item) => c.expr(e, { ...c.ctx, value: acc, this: item }), c.expr(i));
    },
    
    // Logical
    $and: (a, c) => a.every(x => c.expr(x)),
    $or: (a, c) => a.some(x => c.expr(x)),
    $not: (a, c) => !c.expr(a[0]),
    
    // Conditional
    $cond: ([cond, t, f], c) => c.expr(cond) ? c.expr(t) : c.expr(f),
    $ifNull: ([e, d], c) => {
        const v = c.expr(e);
        return v != null ? v : c.expr(d);
    },
    $switch: (a, c) => {
        const cfg = a[0];
        if (!cfg?.branches || !Array.isArray(cfg.branches)) {
            throw new Error('$switch requires {branches: [...], default: ...}');
        }
        for (const b of cfg.branches) {
            if (!b.case || !b.then) throw new Error('Branch needs {case, then}');
            if (c.expr(b.case)) return c.expr(b.then);
        }
        return cfg.default !== undefined ? c.expr(cfg.default) : null;
    },
    
    // Type
    $type: (a, c) => {
        const v = c.expr(a[0]);
        if (v === null) return 'null';
        if (v === undefined) return 'undefined';
        if (Array.isArray(v)) return 'array';
        if (v instanceof Date) return 'date';
        return typeof v;
    },
    
    // Date/Time.
    // A date can reach the engine as a Date, as epoch milliseconds or as the ISO
    // text the SQL adapters store, so every part is read through `asDate()` -
    // otherwise a plain 'YYYY-MM-DD' string silently becomes null here while the
    // SQL backends extract the value out of it.
    $year: (a, c) => part(asDate(c.expr(a[0])), (d) => d.getFullYear()),
    $month: (a, c) => part(asDate(c.expr(a[0])), (d) => d.getMonth() + 1),
    $dayOfMonth: (a, c) => part(asDate(c.expr(a[0])), (d) => d.getDate()),
    $hour: (a, c) => part(asDate(c.expr(a[0])), (d) => d.getHours()),
    $minute: (a, c) => part(asDate(c.expr(a[0])), (d) => d.getMinutes()),
    $second: (a, c) => part(asDate(c.expr(a[0])), (d) => d.getSeconds()),
    $millisecond: (a, c) => part(asDate(c.expr(a[0])), (d) => d.getMilliseconds()),
    // Monday is 1, matching `EXTRACT(DOW) + 1` / `DAYOFWEEK()` / `strftime('%w')+1`.
    $dayOfWeek: (a, c) => part(asDate(c.expr(a[0])), (d) => d.getDay() + 1),
    // Week of the year, weeks starting on Monday and the first week being the one
    // holding the first Monday - i.e. SQLite's `strftime('%W')`, which the other
    // dialects are compared against in the unified tests.
    $week: (a, c) => part(asDate(c.expr(a[0])), (d) => {
        const first = new Date(d.getFullYear(), 0, 1);
        const daysToFirstMonday = (8 - first.getDay()) % 7;
        const dayOfYear = Math.floor((d - first) / 86400000);
        return dayOfYear < daysToFirstMonday ? 0 : Math.floor((dayOfYear - daysToFirstMonday) / 7) + 1;
    }),
    
    // Type conversion
    $toString: (a, c) => String(c.expr(a[0])),
    $toInt: (a, c) => parseInt(c.expr(a[0])),
    $toDouble: (a, c) => parseFloat(c.expr(a[0])),
    $toBool: (a, c) => Boolean(c.expr(a[0])),
    // `new Date(undefined)` is an Invalid Date; the SQL backends yield NULL, and
    // an Invalid Date poisons every later comparison, so go through asDate().
    $toDate: (a, c) => asDate(c.expr(a[0])),
    
    // Literal
    $literal: (a) => a[0],
};

// ============================================
// UPDATE OPERATORS (~1.5 KB)
// ============================================

export const updateOps = {
    $set: (f, doc) => Object.entries(f).forEach(([k, v]) => setPath(doc, k, v)),
    $unset: (f, doc) => Object.keys(f).forEach(k => delPath(doc, k)),
    $inc: (f, doc) => Object.entries(f).forEach(([k, v]) => setPath(doc, k, (getPath(doc, k) || 0) + v)),
    $mul: (f, doc) => Object.entries(f).forEach(([k, v]) => setPath(doc, k, (getPath(doc, k) || 0) * v)),
    
    $min: (f, doc) => Object.entries(f).forEach(([k, v]) => {
        const curr = getPath(doc, k);
        if (curr === undefined || v < curr) setPath(doc, k, v);
    }),
    
    $max: (f, doc) => Object.entries(f).forEach(([k, v]) => {
        const curr = getPath(doc, k);
        if (curr === undefined || v > curr) setPath(doc, k, v);
    }),
    
    $push: (f, doc) => Object.entries(f).forEach(([k, v]) => {
        const curr = getPath(doc, k);
        if (Array.isArray(curr)) {
            if (isObject(v) && v.$each) {
                const { $each: items, $slice: sl, $sort: srt, $position: pos } = v;
                
                if (pos !== undefined) curr.splice(pos, 0, ...items);
                else curr.push(...items);
                
                if (srt) {
                    if (srt === 1) curr.sort((a, b) => a > b ? 1 : -1);
                    else if (srt === -1) curr.sort((a, b) => a < b ? 1 : -1);
                    else if (isObject(srt)) {
                        const [sk, ord] = Object.entries(srt)[0];
                        curr.sort((a, b) => {
                            const av = getPath(a, sk);
                            const bv = getPath(b, sk);
                            return (av > bv ? 1 : av < bv ? -1 : 0) * ord;
                        });
                    }
                }
                
                if (sl !== undefined) {
                    const sliced = sl < 0 ? curr.slice(sl) : curr.slice(0, sl);
                    curr.length = 0;
                    curr.push(...sliced);
                }
            } else {
                curr.push(v);
            }
        } else {
            setPath(doc, k, [v]);
        }
    }),
    
    // `$pull` removes the elements that *match*; the scalar branch used to return
    // "does not match" and was then negated again by the filter below, so it
    // removed every element except the one that was asked to be pulled.
    // Called as `(fields, doc, isInsert, filter)` like every other update operator -
    // declaring `filter` in the third slot used to receive the `isInsert` flag.
    $pull: (f, doc, isInsert, filter) => Object.entries(f).forEach(([k, cond]) => {
        const curr = getPath(doc, k);
        if (Array.isArray(curr)) {
            const matches = isObject(cond) ? filter(cond) : (i) => deepEquals(i, cond);
            setPath(doc, k, curr.filter(i => !matches(i)));
        }
    }),
    
    $pullAll: (f, doc) => Object.entries(f).forEach(([k, vals]) => {
        const curr = getPath(doc, k);
        if (Array.isArray(curr) && Array.isArray(vals)) {
            setPath(doc, k, curr.filter(i => !vals.some(v => deepEquals(v, i))));
        }
    }),
    
    $addToSet: (f, doc) => Object.entries(f).forEach(([k, v]) => {
        const curr = getPath(doc, k);
        if (Array.isArray(curr)) {
            if (isObject(v) && v.$each) {
                v.$each.forEach(item => {
                    if (!curr.some(e => deepEquals(e, item))) curr.push(item);
                });
            } else {
                if (!curr.some(i => deepEquals(i, v))) curr.push(v);
            }
        } else {
            setPath(doc, k, [v]);
        }
    }),
    
    $pop: (f, doc) => Object.entries(f).forEach(([k, v]) => {
        const curr = getPath(doc, k);
        if (Array.isArray(curr)) {
            if (v === 1) curr.pop();
            else if (v === -1) curr.shift();
        }
    }),
    
    $rename: (f, doc) => Object.entries(f).forEach(([old, neu]) => {
        const v = getPath(doc, old);
        if (v !== undefined) {
            setPath(doc, neu, v);
            delPath(doc, old);
        }
    }),
    
    $currentDate: (f, doc) => Object.entries(f).forEach(([k]) => setPath(doc, k, new Date())),
    
    $setOnInsert: (f, doc, isInsert) => {
        if (isInsert) Object.entries(f).forEach(([k, v]) => setPath(doc, k, v));
    },
};

// ============================================
// STAGE OPERATORS (~2.5 KB)
// ============================================

export const stageOps = {
    $match: (args, ctx, filter) => filter(args)(ctx) ? ctx : null,
    
    $project: (proj, ctx, expression) => {
        const res = {};
        const excl = Object.entries(proj).filter(([_, v]) => v === 0 || v === false).map(([k]) => k);
        
        if (excl.length > 0) {
            Object.keys(ctx).forEach(k => {
                if (!excl.includes(k)) res[k] = getPath(ctx, k);
            });
        } else {
            Object.entries(proj).forEach(([k, v]) => {
                if (v === 1 || v === true) res[k] = getPath(ctx, k);
                else if (isObject(v) || is$(v)) res[k] = expression(v)(ctx);
                else res[k] = v;
            });
            
            if (!('_id' in proj) && '_id' in ctx) res._id = ctx._id;
            else if (proj._id === 0 || proj._id === false) delete res._id;
        }
        
        return res;
    },
    
    $addFields: (fields, ctx, expression) => {
        const res = { ...ctx };
        Object.entries(fields).forEach(([k, v]) => {
            res[k] = expression(v)(ctx);
        });
        return res;
    },
    
    $set: (fields, ctx, expression) => stageOps.$addFields(fields, ctx, expression),
    
    $unset: (fields, ctx) => {
        const res = { ...ctx };
        const keys = Array.isArray(fields) ? fields : [fields];
        keys.forEach(k => delPath(res, k));
        return res;
    },
    
    $group: ({ _id, ...acc }, ctxArr, expression) => {
        const grps = ctxArr.reduce((a, i) => {
            // A missing field groups under null - that is what MongoDB does and
            // what the SQL adapters produce (`GROUP BY col` puts NULL rows in one
            // group). `undefined` would survive as an absent `_id` on the result.
            const key = _id === null ? null : expression(_id)(i) ?? null;
            const k = JSON.stringify(key);
            if (!a[k]) a[k] = { _id: key, items: [] };
            a[k].items.push(i);
            return a;
        }, {});
        
        return Object.values(grps).map(g => {
            const res = { _id: g._id };
            
            Object.entries(acc).forEach(([k, accExpr]) => {
                if (!isObject(accExpr)) throw new Error(`Accumulator must be object: ${k}`);
                
                const [op, arg] = Object.entries(accExpr)[0];
                
                switch (op) {
                    case '$sum':
                        res[k] = arg === 1 ? g.items.length : g.items.reduce((s, i) => s + (expression(arg)(i) || 0), 0);
                        break;
                    case '$avg':
                        const vals = g.items.map(i => expression(arg)(i) || 0);
                        res[k] = vals.length > 0 ? vals.reduce((a, b) => a + b, 0) / vals.length : null;
                        break;
                    case '$min':
                        res[k] = Math.min(...g.items.map(i => expression(arg)(i)).filter(v => v != null));
                        break;
                    case '$max':
                        res[k] = Math.max(...g.items.map(i => expression(arg)(i)).filter(v => v != null));
                        break;
                    case '$push':
                        res[k] = g.items.map(i => expression(arg)(i));
                        break;
                    case '$addToSet':
                        const uniq = [];
                        const seen = new Set();
                        g.items.forEach(i => {
                            const v = expression(arg)(i);
                            const sk = JSON.stringify(v);
                            if (!seen.has(sk)) {
                                seen.add(sk);
                                uniq.push(v);
                            }
                        });
                        res[k] = uniq;
                        break;
                    case '$first':
                        res[k] = g.items.length > 0 ? expression(arg)(g.items[0]) : null;
                        break;
                    case '$last':
                        res[k] = g.items.length > 0 ? expression(arg)(g.items[g.items.length - 1]) : null;
                        break;
                    default:
                        throw new Error(`Unknown accumulator: ${op}`);
                }
            });
            
            return res;
        });
    },
    
    $sort: (sObj, ctxArr) => [...ctxArr].sort((a, b) => {
        for (const [k, ord] of Object.entries(sObj)) {
            const av = getPath(a, k);
            const bv = getPath(b, k);
            if (av !== bv) {
                if (av == null) return 1;
                if (bv == null) return -1;
                return (av > bv ? 1 : -1) * ord;
            }
        }
        return 0;
    }),
    
    $limit: (cnt, ctxArr) => ctxArr.slice(0, cnt),
    $skip: (cnt, ctxArr) => ctxArr.slice(cnt),
    $count: (field, ctxArr) => [{ [field]: ctxArr.length }],
    
    $unwind: (cfg, ctxArr) => {
        const path = typeof cfg === 'string' ? cfg.replace(/^\$/, '') : cfg.path.replace(/^\$/, '');
        const preserve = isObject(cfg) && cfg.preserveNullAndEmptyArrays;
        const idx = isObject(cfg) ? cfg.includeArrayIndex : null;
        
        const res = [];
        ctxArr.forEach(doc => {
            const arr = getPath(doc, path);
            if (Array.isArray(arr) && arr.length > 0) {
                arr.forEach((item, i) => {
                    const n = clone(doc);
                    setPath(n, path, item);
                    if (idx) setPath(n, idx, i);
                    res.push(n);
                });
            } else if (preserve) {
                const n = clone(doc);
                setPath(n, path, null);
                res.push(n);
            }
        });
        
        return res;
    },
    
    $bucket: (cfg, ctxArr, expression) => {
        const { groupBy, boundaries, default: def, output } = cfg;
        if (!Array.isArray(boundaries) || boundaries.length < 2) throw new Error('$bucket requires boundaries');
        
        const getBucket = (v) => {
            for (let i = 0; i < boundaries.length - 1; i++) {
                if (v >= boundaries[i] && v < boundaries[i + 1]) return boundaries[i];
            }
            return def;
        };
        
        const groups = {};
        ctxArr.forEach(i => {
            const v = expression(groupBy)(i);
            const b = getBucket(v);
            const k = JSON.stringify(b);
            if (!groups[k]) groups[k] = { _id: b, items: [] };
            groups[k].items.push(i);
        });
        
        const res = Object.values(groups).map(g => {
            const out = { _id: g._id, count: g.items.length };
            if (output && isObject(output)) {
                Object.entries(output).forEach(([k, acc]) => {
                    if (!isObject(acc)) throw new Error('Output accumulator must be object');
                    const [op, arg] = Object.entries(acc)[0];
                    switch (op) {
                        case '$count':
                            out[k] = arg === 1 ? g.items.length : g.items.filter(i => expression(arg)(i) != null).length;
                            break;
                        case '$sum':
                            out[k] = g.items.reduce((s, i) => s + (expression(arg)(i) || 0), 0);
                            break;
                        case '$avg':
                            const vals = g.items.map(i => expression(arg)(i) || 0);
                            out[k] = vals.length > 0 ? vals.reduce((a, b) => a + b, 0) / vals.length : null;
                            break;
                        case '$min':
                            out[k] = Math.min(...g.items.map(i => expression(arg)(i)).filter(v => v != null));
                            break;
                        case '$max':
                            out[k] = Math.max(...g.items.map(i => expression(arg)(i)).filter(v => v != null));
                            break;
                        case '$push':
                            out[k] = g.items.map(i => expression(arg)(i));
                            break;
                        case '$addToSet':
                            const uniq = [];
                            const seen = new Set();
                            g.items.forEach(i => {
                                const v = expression(arg)(i);
                                const sk = JSON.stringify(v);
                                if (!seen.has(sk)) {
                                    seen.add(sk);
                                    uniq.push(v);
                                }
                            });
                            out[k] = uniq;
                            break;
                        default:
                            throw new Error(`Unknown accumulator: ${op}`);
                    }
                });
            }
            return out;
        });
        
        return res;
    },
    
    $sortByCount: (expr, ctxArr, expression) => {
        const keyFn = typeof expr === 'string' && expr.startsWith('$') ? (i) => getPath(i, expr.slice(1)) : (i) => expression(expr)(i);
        const counts = ctxArr.reduce((a, i) => {
            // Same rule as `$group`: a missing field counts towards the null key
            // (and `JSON.stringify(undefined)` is not a string, so the object key
            // would not even round-trip through JSON.parse below).
            const k = JSON.stringify(keyFn(i) ?? null);
            a[k] = (a[k] || 0) + 1;
            return a;
        }, {});
        
        const res = Object.entries(counts).map(([k, v]) => ({ _id: JSON.parse(k), count: v }));
        return res.sort((a, b) => b.count - a.count);
    },
};

// ============================================
// CORE ENGINE (~2 KB)
// ============================================

export const createMemoryDB = ({ filterOps: fOps = filterOps, exprOps: eOps = exprOps, updateOps: uOps = updateOps, stageOps: sOps = stageOps } = {}) => {
    // FILTER
    const filter = (q) => (doc) => {
        if (!isObject(q)) throw new Error('Query must be object');
        
        const evalCond = (key, cond, value) => {
            if (isObject(cond)) {
                return Object.entries(cond).every(([op, arg]) => {
                    if (is$(op)) {
                        if (!fOps[op]) throw new Error(`Unknown filter operator: ${op}`);
                        return fOps[op](arg, value, filter, expression);
                    }
                    return deepEquals(cond, value);
                });
            }
            return deepEquals(cond, value);
        };
        
        return Object.entries(q).every(([k, v]) => {
            if (is$(k)) {
                if (!fOps[k]) throw new Error(`Unknown filter operator: ${k}`);
                return fOps[k](v, doc, filter, expression);
            }
            const val = getPath(doc, k);
            return evalCond(k, v, val);
        });
    };
    
    // EXPRESSION
    const expression = (e, ctx = {}) => (doc) => {
        if (isObject(e)) {
            const [op, args] = Object.entries(e)[0];
            if (!eOps[op]) throw new Error(`Unknown expression operator: ${op}`);
            return eOps[op](Array.isArray(args) ? args : [args], { expr: (x) => expression(x)(doc), ctx, doc });
        }
        if (is$(e)) return getPath(doc, e.slice(1));
        return e;
    };
    
    // PROJECT
    const project = (doc, proj) => {
        if (!proj) return doc;
        
        const res = {};
        const hasInc = Object.values(proj).some(v => v === 1 || v === true);
        
        if (hasInc) {
            Object.entries(proj).forEach(([k, v]) => {
                if (v === 1 || v === true) {
                    const val = getPath(doc, k);
                    if (val !== undefined) setPath(res, k, val);
                } else if (isObject(v) || is$(v)) {
                    res[k] = expression(v)(doc);
                }
            });
            
            if (!('_id' in proj) || proj._id !== 0) {
                if (doc._id !== undefined) res._id = doc._id;
            }
        } else {
            Object.assign(res, doc);
            Object.entries(proj).forEach(([k, v]) => {
                if (v === 0 || v === false) delPath(res, k);
            });
        }
        
        return res;
    };
    
    // AGGREGATE
    const aggregate = (pipe) => (coll) => {
        if (!Array.isArray(pipe)) throw new Error('Pipeline must be array');
        
        let ctxArr = Array.isArray(coll) ? coll : [coll];
        
        for (const stage of pipe) {
            const [op, args] = Object.entries(stage)[0];
            if (!sOps[op]) throw new Error(`Unknown stage: ${op}`);
            if (op === '$group' || op === '$bucket' || op === '$sortByCount') {
                ctxArr = sOps[op](args, ctxArr, expression);
            } else if (op === '$match') {
                ctxArr = ctxArr.filter(i => sOps[op](args, i, filter));
            } else if (op === '$project' || op === '$addFields' || op === '$set') {
                ctxArr = ctxArr.map(i => sOps[op](args, i, expression));
            } else {
                ctxArr = sOps[op](args, ctxArr);
            }
        }
        
        return ctxArr;
    };
    
    // FIND QUERY CLASS
    class FindQuery {
        constructor(data, query, projection) {
            this._data = data;
            this._query = query;
            this._proj = projection;
            this._sort = null;
            this._skip = 0;
            this._limit = null;
            this._dist = false;
            this._cnt = false;
        }
        
        sort(obj) { this._sort = obj; return this; }
        skip(n) { this._skip = n; return this; }
        limit(n) { this._limit = n; return this; }
        distinct() { this._dist = true; return this; }
        count() { this._cnt = true; return this; }
        
        toArray() {
            let res = this._data.filter(filter(this._query));
            if (this._sort) {
                const sObj = this._sort;
                res = [...res].sort((a, b) => {
                    for (const [k, ord] of Object.entries(sObj)) {
                        const av = getPath(a, k);
                        const bv = getPath(b, k);
                        if (av !== bv) {
                            if (av == null) return 1;
                            if (bv == null) return -1;
                            return (av > bv ? 1 : -1) * ord;
                        }
                    }
                    return 0;
                });
            }
            if (this._skip) res = res.slice(this._skip);
            if (this._limit != null) res = res.slice(0, this._limit);
            if (this._cnt) return [{ count: res.length }];
            
            if (this._dist) {
                const seen = new Set();
                res = res.filter(d => {
                    const k = JSON.stringify(d);
                    if (seen.has(k)) return false;
                    seen.add(k);
                    return true;
                });
            }
            
            return res;
        }
        
        toString() {
            return JSON.stringify({
                query: this._query,
                projection: this._proj,
                sort: this._sort,
                skip: this._skip,
                limit: this._limit
            });
        }
    }
    
    // COLLECTION CLASS
    class Collection {
        constructor(name, data = [], options = {}) {
            this.name = name;
            this._data = data;
            this._id = data.length > 0
                ? Math.max(...data.map(d => d._id || 0).filter(id => typeof id === 'number')) + 1
                : 1;
            this.options = options || {};
            this.idColumn = this.options.idColumn || '_id';
            this.idStrategy = this.options.idStrategy || 'auto';
            this.idGenerator = typeof this.options.idGenerator === 'function' ? this.options.idGenerator : null;
        }
        
        _genId() { return this._id++; }
        _genMongoId() {
            const ts = Math.floor(Date.now() / 1000).toString(16).padStart(8, '0');
            const rand = Array.from({ length: 16 }, () => Math.floor(Math.random() * 16).toString(16)).join('');
            return (ts + rand).slice(0, 24);
        }
        
        find(q = {}, proj = null, options = null) {
            const fq = new FindQuery(this._data, q, proj);
            if (options && typeof options === 'object') {
                if (options.sort) fq.sort(options.sort);
                if (options.skip != null) fq.skip(options.skip);
                if (options.limit != null) fq.limit(options.limit);
                if (options.distinct) fq.distinct();
            }
            return fq;
        }
        
        findOne(q = {}, proj = null) {
            const res = this.find(q, proj).limit(1).toArray();
            return res.length > 0 ? res[0] : null;
        }
        
        findById(id) {
            return this.findOne({ _id: id });
        }
        
        insertOne(doc) {
            const n = clone(doc);
            const idCol = this.idColumn;
            const nonAuto = this.idStrategy !== 'auto';
            if (nonAuto) {
                if (n[idCol] == null) {
                    n[idCol] = this.idGenerator ? this.idGenerator() : (this.idStrategy === 'mongo' ? this._genMongoId() : n[idCol]);
                }
            } else {
                if (idCol === '_id') {
                    if (n._id == null) n._id = this._genId();
                } else {
                    if (n[idCol] == null) n[idCol] = this._genId();
                }
            }
            this._data.push(n);
            return { insertedId: n[idCol], acknowledged: true };
        }
        
        insertMany(docs) {
            if (!Array.isArray(docs)) throw new Error('insertMany requires array');
            const ids = docs.map(d => this.insertOne(d).insertedId);
            return { insertedIds: ids, acknowledged: true };
        }
        
        updateOne(q, upd, opts = {}) {
            const idx = this._data.findIndex(filter(q));
            
            if (idx === -1) {
                if (opts.upsert) {
                    const n = {};
                    Object.entries(upd).forEach(([op, f]) => {
                        if (is$(op)) uOps[op](f, n, true, filter);
                        else setPath(n, op, f);
                    });
                    this.insertOne({ ...q, ...n });
                    return { modifiedCount: 0, upsertedCount: 1, acknowledged: true };
                }
                return { modifiedCount: 0, matchedCount: 0, acknowledged: true };
            }
            
            const c = clone(this._data[idx]);
            Object.entries(upd).forEach(([op, f]) => {
                if (is$(op)) {
                    if (!uOps[op]) throw new Error(`Unknown update operator: ${op}`);
                    uOps[op](f, c, false, filter);
                } else {
                    setPath(c, op, f);
                }
            });
            
            this._data[idx] = c;
            return { modifiedCount: 1, matchedCount: 1, acknowledged: true };
        }
        
        updateMany(q, upd, opts = {}) {
            let mod = 0;
            this._data.forEach((doc, i) => {
                if (filter(q)(doc)) {
                    const c = clone(doc);
                    Object.entries(upd).forEach(([op, f]) => {
                        if (is$(op)) {
                            if (!uOps[op]) throw new Error(`Unknown update operator: ${op}`);
                            uOps[op](f, c, false, filter);
                        } else {
                            setPath(c, op, f);
                        }
                    });
                    this._data[i] = c;
                    mod++;
                }
            });
            return { modifiedCount: mod, matchedCount: mod, acknowledged: true };
        }
        
        replaceOne(q, repl, opts = {}) {
            const idx = this._data.findIndex(filter(q));
            if (idx === -1) {
                if (opts.upsert) {
                    this.insertOne(repl);
                    return { modifiedCount: 0, upsertedCount: 1, acknowledged: true };
                }
                return { modifiedCount: 0, matchedCount: 0, acknowledged: true };
            }
            const _id = this._data[idx]._id;
            this._data[idx] = { ...repl, _id };
            return { modifiedCount: 1, matchedCount: 1, acknowledged: true };
        }
        
        deleteOne(q) {
            const idx = this._data.findIndex(filter(q));
            if (idx === -1) return { deletedCount: 0, acknowledged: true };
            this._data.splice(idx, 1);
            return { deletedCount: 1, acknowledged: true };
        }
        
        deleteMany(q = {}) {
            const init = this._data.length;
            for (let i = this._data.length - 1; i >= 0; i--) {
                if (filter(q)(this._data[i])) this._data.splice(i, 1);
            }
            return { deletedCount: init - this._data.length, acknowledged: true };
        }
        
        countDocuments(q = {}) {
            return this._data.filter(filter(q)).length;
        }
        
        estimatedDocumentCount() {
            return this._data.length;
        }
        
        distinct(field, q = {}) {
            const vals = this._data.filter(filter(q)).map(d => getPath(d, field)).filter(v => v !== undefined);
            const uniq = [];
            const seen = new Set();
            vals.forEach(v => {
                const k = JSON.stringify(v);
                if (!seen.has(k)) {
                    seen.add(k);
                    uniq.push(v);
                }
            });
            return uniq;
        }
        
        aggregate(pipe) {
            return aggregate(pipe)(this._data);
        }
        
        drop() {
            this._data.length = 0;
            this._id = 1;
            return { acknowledged: true };
        }
        
        getAll() {
            return [...this._data];
        }
        
        size() {
            return this._data.length;
        }
    }
    
    // DATABASE CLASS
    class Database {
        constructor(name) {
            this.name = name;
            this.collections = new Map();
        }
        
        collection(name, initData, options) {
            if (!this.collections.has(name)) {
                this.collections.set(name, new Collection(name, initData || [], options || {}));
            }
            return this.collections.get(name);
        }
        
        dropCollection(name) {
            return this.collections.delete(name);
        }
        
        listCollections() {
            return Array.from(this.collections.keys());
        }
        
        stats(name) {
            const coll = this.collections.get(name);
            if (!coll) return null;
            
            const data = coll.getAll();
            const size = JSON.stringify(data).length;
            
            return {
                name: coll.name,
                count: coll.size(),
                size,
                avgObjSize: coll.size() > 0 ? size / coll.size() : 0
            };
        }
        
        dropDatabase() {
            this.collections.clear();
            return { acknowledged: true };
        }
    }
    
    // EXTENSION & EXPORTS
    const extend = {
        filter: (ops) => Object.assign(fOps, ops),
        expression: (ops) => Object.assign(eOps, ops),
        update: (ops) => Object.assign(uOps, ops),
        stage: (ops) => Object.assign(sOps, ops),
    };
    
    const db = (name) => new Database(name);
    const collection = (name, initData = [], options) => new Collection(name, initData, options);
    
    return {
        filter,
        expression,
        aggregate,
        project,
        collection,
        FindQuery,
        extend,
        db,
        Database,
        Collection,
    };
};

// ============================================
// DEFAULT EXPORTS (Full Version)
// ============================================

const fullMemoryDB = createMemoryDB({
    filterOps,
    exprOps,
    updateOps,
    stageOps,
});

export const { filter, expression, aggregate, project, collection, FindQuery, extend, db, Database, Collection } = fullMemoryDB;

export default collection;

// Re-export utilities
export { deepEquals, getPath, setPath, delPath as deletePath, clone };