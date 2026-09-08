import { createMongoSchemaless } from '../src/client.js';
import { loadEnv } from '../src/env.js';

await loadEnv();

(async () => {
  const { adapter, client } = await createMongoSchemaless({
    host: process.env.MONGO_HOST,
    port: process.env.MONGO_PORT ? parseInt(process.env.MONGO_PORT) : undefined,
    user: process.env.MONGO_USER,
    password: process.env.MONGO_PASSWORD,
    database: process.env.MONGO_DB || 'test_database',
  });
  const users = adapter.collection('users');
  await adapter.dropCollection('users').catch(()=>{});
  await users.insertOne({ name: 'Alice', age: 25, profile: { score: 85 } });
  await users.updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 5 } });
  const rows = await users.find({ name: 'Alice' }).toArray();
  console.log('score', rows.map(r => r.profile?.score));
  console.log('estimatedDocumentCount', await users.estimatedDocumentCount());
  await client.close();
})();