import { createSchemalessClient } from '../src/client.js';
import { loadEnv } from '../src/env.js';

await loadEnv();

(async () => {
  const client = await createSchemalessClient('mysql', {
    host: process.env.MYSQL_HOST,
    user: process.env.MYSQL_USER,
    password: process.env.MYSQL_PASS,
    database: process.env.MYSQL_DB,
  });

  const users = client.db('app').collection('users');
  await client.raw.query('DROP TABLE IF EXISTS users');
  await users.insertOne({ name: 'Alice', age: 25, profile: { score: 85 } });
  await users.updateOne({ name: 'Alice' }, { $set: { 'profile.score': 90 } });

  // `client.raw` is the underlying mysql2/promise connection.
  const [rows] = await client.raw.query("SELECT JSON_EXTRACT(profile, '$.score') AS score FROM users WHERE name = 'Alice'");
  console.log('score', rows);
  console.log('estimatedDocumentCount', await users.estimatedDocumentCount());

  await client.close();
})();
