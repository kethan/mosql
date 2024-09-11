// Helper function to compare equality of objects
const isObject = (obj) => typeof obj === 'object' && !Array.isArray(obj);
const is$ = (obj) => typeof obj === 'string' && obj.startsWith('$');

const jsonPath = (path = '', db = 'sqlite') => {
    let [column, ...rest] = path.split('.');
    if (!rest.length) return path;
    let l = (`$.${rest.join('.').replace(/(\d+)/g, '[$1]')}`).replace(/\.\[/g, '[');
    return db == 'pg' ? `(${column}::json #> {${rest.join('.').replace(/\./g, ',')}})` : `json_extract(${column}, '${l}')`
}

const filterOps = {
    $eq: (value, db) => `= ${value}`,
    $ne: (value, db) => `!= ${value}`,
    $gt: (value, db) => `> ${value}`,
    $gte: (value, db) => `>= ${value}`,
    $lt: (value, db) => `< ${value}`,
    $lte: (value, db) => `<= ${value}`,
    $in: (value, db) => `IN (${value.join(', ')})`,
    $nin: (value, db) => `NOT IN (${value.join(', ')})`,
    $and: (value, db) => `(${value.map(subFilter => filter(subFilter, db)).join(' AND ')})`,
    $or: (value, db) => `(${value.map(subFilter => filter(subFilter, db)).join(' OR ')})`,
    $not: (value, db) => `NOT (${filter(value)})`,

    $exists: (value, db) => value ? 'IS NOT NULL' : 'IS NULL',
    $expr: (value, db) => expression(value, db),
    $size: (value, db) => {
        if (db === 'sqlite') return `json_array_length(${value})`;
    },
    $toInt: (value, db) => db === 'pg' ? `(${filter(value, db)})::int` : '',
    $like: (value, db) => `LIKE ${value}`,
    $ilike: (value, db) => `LIKE LOWER(${value})`,
    $nlike: (value, db) => `NOT LIKE ${value}`,
    $nilike: (value, db) => `NOT LIKE LOWER(${value})`,
    // $all: (value, db) => `@todo`, // Need context to handle arrays properly in SQL

    // $elemMatch: (value, db, field) => {
    //     // console.log(field);
    //     if (db === 'sqlite') {
    //         const conditions = Object.entries(value).map(([key, cond]) => {
    //             const [op, val] = Object.entries(cond)[0];
    //             const type = typeof val === 'number' ? 'integer' :
    //                 typeof val === 'boolean' ? 'boolean' :
    //                     'text'; // default to text
    //             return `(type = '${type}' AND value ${filterOps[op](val, db)})`;
    //         }).join(' AND ');
    //         return `EXISTS (SELECT 1 FROM json_each(${field}, '$') WHERE ${conditions})`;
    //     }
    //     // Add support for PostgreSQL if needed
    // },
    // $all: (value, db, field) => {
    //     // console.log(value, db, field);
    //     if (db === 'sqlite') {
    //         const conditions = value.map(subCondition => {
    //             if (isObject(subCondition) && subCondition.hasOwnProperty('$elemMatch')) {
    //                 // console.log('sub', subCondition);
    //                 return filterOps.$elemMatch(subCondition.$elemMatch, db, field);
    //             }
    //             return filter(subCondition, db, field);
    //         }).join(' AND ');
    //         return `(${conditions})`;
    //     }
    //     // Add support for PostgreSQL if needed
    // }
}

const expressionOps = {
    $add: (args, db) => args.map(arg => expression(arg, db)).join(' + '),
    $subtract: (args, db) => args.map(arg => expression(arg, db)).join(' - '),
    $multiply: (args, db) => args.map(arg => expression(arg, db)).join(' * '),
    $divide: (args, db) => args.map(arg => expression(arg, db)).join(' / '),

    $concat: (args, db) => `CONCAT(${args.map(arg => expression(arg, db)).join(', ')})`,

    $min: (args, db) => `MIN(${args.map(arg => expression(arg, db)).join(', ')})`,
    $max: (args, db) => `MAX(${args.map(arg => expression(arg, db)).join(', ')})`,
    $avg: (args, db) => `AVG(${args.map(arg => expression(arg, db)).join(', ')})`,
    $sum: (args, db) => `SUM(${args.map(arg => expression(arg, db)).join(', ')})`,

    $cond: (args, db) => `CASE WHEN ${expression(args[0], db)} THEN ${expression(args[1], db)} ELSE ${expression(args[2], db)} END`,

    $eq: (args, db) => `${expression(args[0], db)} = ${expression(args[1], db)}`,
    $ne: (args, db) => `${expression(args[0], db)} <> ${expression(args[1], db)}`,
    $gt: (args, db) => `${expression(args[0], db)} > ${expression(args[1], db)}`,
    $gte: (args, db) => `${expression(args[0], db)} >= ${expression(args[1], db)}`,
    $lt: (args, db) => `${expression(args[0], db)} < ${expression(args[1], db)}`,
    $lte: (args, db) => `${expression(args[0], db)} <= ${expression(args[1], db)}`,

    $in: (args, db) => `${expression(args[0], db)} IN (${args[1].map(arg => expression(arg, db)).join(', ')})`,
    $nin: (args, db) => `${expression(args[0], db)} NOT IN (${args[1].map(arg => expression(arg, db)).join(', ')})`,
    $and: (args, db) => args.map(arg => `(${expression(arg, db)})`).join(' AND '),
    $or: (args, db) => args.map(arg => `(${expression(arg, db)})`).join(' OR '),
    $not: (args, db) => `NOT (${expression(args[0], db)})`,
    $switch: (args, db) => 'CASE ' + args[0].branches.map(branch => `WHEN ${expression(branch.case, db)} THEN ${expression(branch.then, db)}`).join(' ') + ` ELSE ${expression(args[0].default, db)} END`,

    $toInt: (args, db) => db === 'pg' ? `(${expression(args[0], db)})::int` : '',
    $exists: (args, db) => `${expression(args[0])} ${args[1] ? 'IS NOT NULL' : 'IS NULL'}`,
    // $elemMatch: (args, db, options) => `@todo`
};

const filter = (query, db = 'sqlite', options) => {
    if (typeof query === 'object' && !Array.isArray(query)) {
        return Object.entries(query).map(([key, subFilter]) => {
            const field = key.includes('.') ? jsonPath(key, db) : key;

            if (typeof key === 'string' && key.startsWith('$')) {
                return filterOps[key](typeof subFilter === 'string' ? `'${subFilter}'` : subFilter, db, field);
            }

            if (typeof subFilter === 'string') return `${field} = '${subFilter}'`;

            if (filterOps.$elemMatch && key === '$elemMatch') {
                return filterOps.$elemMatch(subFilter, db, field);
            }

            return `${field} ${typeof subFilter === 'object' ? filter(subFilter, db, options) : filterOps['$eq'](subFilter, db, field)}`;
        }).join(' AND ');
    }

    return query;
};

// expression evalation based on context
const expression = (expr, db = 'sqlite') => {
    if (is$(expr)) {
        const value = expr.slice(1);
        return value.includes('.') ? jsonPath(value, db) : value;
    }

    if (isObject(expr)) {
        const [operator, args] = Object.entries(expr)[0];

        if (expressionOps[operator]) {
            return `(${expressionOps[operator](Array.isArray(args) ? args : [args], db, expr)})`;
        }
    }

    if (typeof expr === 'string' && expr.includes('.')) {
        return jsonPath(expr, db);
    }

    return typeof expr === 'string' ? `'${expr}'` : expr;
};


// const add = (op, fn) => aggregateOps[op] = fn;

const add = (which, op, fn) => {
    if (which === 'filter') filterOps[op] = fn;
    // if (which === 'stage') stagesOps[op] = fn;
    if (which === 'expression') expressionOps[op] = fn;
}

const aggregate = (pipeline) => (from, db) => {
    let sql = `SELECT * FROM ${from}`;
    let groupBy = '';
    let having = '';

    pipeline.forEach(stage => {
        const [operator, args] = Object.entries(stage)[0];
        switch (operator) {
            case '$project':
                sql = `SELECT ${Object.entries(args).map(([key, value]) => {
                    if (is$(value)) {
                        return `${jsonPath(value.slice(1), db)} AS ${key}`;
                    } else if (isObject(value)) {
                        return `${expression(value, db)} AS ${key}`;
                    } else if (value === 1) {
                        return key;
                    }
                }).join(', ')} FROM (${sql})`;
                // const allFields = '*';
                // const excludedFields = Object.entries(args).filter(([, v]) => v === 0).map(([k]) => k);
                // const selectedFields = allFields === '*'
                //     ? Object.entries(args).map(([key, value]) => {
                //         if (value === 0) return '';
                //         return is$(value) ? `${jsonPath(value.slice(1), db)} AS ${key}`
                //             : isObject(value) ? `${expression(value, db)} AS ${key}`
                //                 : value === 1 ? key : '';
                //     }).filter(Boolean).join(', ') || '*'
                //     : allFields.filter(f => !excludedFields.includes(f)).join(', ');

                // sql = `SELECT ${selectedFields} FROM (${sql})`;
                break;
            case '$match':
                if (groupBy) {
                    having = `HAVING ${filter(args, db)}`;
                } else {
                    sql += ` WHERE ${filter(args, db)}`;
                }
                break;
            case '$group':
                groupBy = expression(args._id, db);
                const aggregates = Object.entries(args).filter(([key]) => key !== '_id').map(([key, expr]) => {
                    return `${expression(expr, db)} AS ${key}`;
                }).join(', ');
                sql = `SELECT ${groupBy}${aggregates ? ', ' + aggregates : ''} FROM (${sql})`;
                break;
            case '$sort':
                const sortClauses = Object.entries(args).map(([key, order]) => `${expression(key, db)} ${order === 1 ? 'ASC' : 'DESC'}`).join(', ');
                sql += ` ORDER BY ${sortClauses}`;
                break;
            case '$skip':
                sql += ` OFFSET ${args}`;
                break;
            case '$limit':
                sql += ` LIMIT ${args}`;
                break;
            case '$count':
                sql = `SELECT COUNT(*) AS ${args} FROM (${sql})`;
                break;
        }
    });

    if (groupBy) {
        sql += ` GROUP BY ${groupBy}`;
    }

    if (having) {
        sql += ` ${having}`;
    }

    return sql;
};

export { filter, expression, aggregate, add, jsonPath };