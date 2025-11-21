import Database from 'better-sqlite3';
import { createSchemalessAdapter } from '../src/schemaless.js';
import { runTest } from './common.js';

const main = async () => {
  const { adapter } = createSchemalessAdapter(new Database(':memory:'), 'sqlite');

  await adapter.dropCollection('fh_users');
  await adapter.createTableWithSchema('fh_users', {
    name: { type: 'TEXT', required: true },
    age: { type: 'INTEGER' },
  });

  const users = adapter.collection('fh_users');
  await users.insertOne({ name: 'Alice', age: 25 });
  await users.insertOne({ name: 'Bob', age: 30 });
  await users.insertOne({ name: 'Charlie', age: 22 });
  await users.insertOne({ name: 'Dave', age: 40 });

  await runTest('find helpers: sort asc limit 2 offset 1', async () => {
    const res = await (await users.find({}, null, { sort: { name: 1 }, limit: 2, skip: 1 })).toArray();
    return res.map(r => ({ name: r.name })).sort((a,b)=>a.name.localeCompare(b.name));
  }, [{ name: 'Bob' }, { name: 'Charlie' }]);

  await runTest('findMany: page 2 size 2 includeTotal', async () => {
    const { items, total, page, pageSize } = await users.findMany({ filter: {}, sort: { age: -1 }, page: 2, pageSize: 2, includeTotal: true });
    return [{ total, page, pageSize, items: items.map(i => i.name).sort((a,b)=>a.localeCompare(b)) }];
  }, [{ total: 4, page: 2, pageSize: 2, items: ['Alice', 'Charlie'] }]);
};

await main();