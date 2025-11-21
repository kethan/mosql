import { drizzle } from 'drizzle-orm/neon-http';
import { neon } from '@neondatabase/serverless';
import dotenv from 'dotenv';

dotenv.config();
const sql = neon(process.env.NEON_HTTP_URL);
const db = drizzle(sql);

(async () => {
  const r = await db.execute('SELECT 1');
  console.log(r);
})();