## MoSQL

# Mongo To SQL Query

[![tests](https://github.com/kethan/mosql/actions/workflows/node.js.yml/badge.svg)](https://github.com/kethan/mosql/actions/workflows/node.js.yml) [![Version](https://img.shields.io/npm/v/umosql.svg?color=success&style=flat-square)](https://www.npmjs.com/package/mosql) [![Badge size](https://deno.bundlejs.com/badge?q=umosql&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql)

[![Version](https://img.shields.io/npm/v/umosql.svg?color=success&style=flat-square)](https://www.npmjs.com/package/mosql/lite) [![Badge size](https://deno.bundlejs.com/badge?q=umosq/lite&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql/lite)


[![Version](https://img.shields.io/npm/v/umosql.svg?color=success&style=flat-square)](https://www.npmjs.com/package/mosql/lite) [![Badge size](https://deno.bundlejs.com/badge?q=umosql/timy&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql/tiny)

---

## 📊 Feature Comparison

| Feature                  | Full               | Lite               | Tiny               |
| ------------------------ | ------------------ | ------------------ | ------------------ |
| **Size**                 | ~600 lines         | ~450 lines         | ~300 lines         |
| **JSON Paths**           | ✅                 | ✅                 | ❌                 |
| **JSON Updates**         | ✅                 | ✅                 | ❌                 |
| **Aggregation**          | ✅                 | ❌                 | ❌                 |
| **Filter Operators**     | ✅ All             | ✅ All             | ✅ All             |
| **Expression Operators** | ✅ All             | ✅ Basic           | ✅ Basic           |
| **Update Operators**     | ✅ All             | ✅ All             | ✅ All             |
| **Collection API**       | ✅                 | ✅                 | ✅                 |
| **FindQuery**            | ✅                 | ✅                 | ✅                 |
| **Extend/Add**           | ✅                 | ✅                 | ✅                 |
| **Multi-DB**             | ✅ PG/MySQL/SQLite | ✅ PG/MySQL/SQLite | ✅ PG/MySQL/SQLite |

## 🎯 When to Use Which Version?

### Use **FULL** when:

- You need JSON field support (`profile.score`)
- You need aggregation pipelines ($group, $match, etc.)
- Complex analytics queries
- MongoDB-to-SQL migration

### Use **LITE** when:

- You need JSON fields but not aggregation
- Medium complexity apps
- REST APIs with JSON columns
- Balance between features and size

### Use **TINY** when:

- Simple CRUD operations only
- No JSON columns needed
- Smallest bundle size required
- Simple web apps or microservices

All three versions are production-ready! 🚀

## License

This project is licensed under the MIT License.
