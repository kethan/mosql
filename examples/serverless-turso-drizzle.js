import { drizzle } from 'drizzle-orm/libsql';
import { createClient } from '@libsql/client';
import dotenv from 'dotenv';
dotenv.config();

const client = createClient({ url: process.env.TURSO_HTTP_URL, authToken: process.env.TURSO_TOKEN });
const db = drizzle(client);

(async () => {
  const r = await client.execute('SELECT 1');
  console.log(r);
})();