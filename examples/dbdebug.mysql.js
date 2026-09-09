import dotenv from 'dotenv';
import mysql from 'mysql2/promise';
dotenv.config();

const cfg = { host: process.env.MYSQL_HOST, port: Number(process.env.MYSQL_PORT || 3306), user: process.env.MYSQL_USER, password: process.env.MYSQL_PASS, database: process.env.MYSQL_DB };

const main = async () => {
  const conn = await mysql.createConnection(cfg);
  await conn.query('DROP TABLE IF EXISTS users');
  await conn.query('CREATE TABLE users (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255), profile JSON)');
  await conn.query('INSERT INTO users (name, profile) VALUES (?, ?)', ['Alice', JSON.stringify({ score: 85 })]);
  await conn.query("UPDATE users SET profile = JSON_SET(COALESCE(profile, '{}'), '$.score', 90) WHERE name = 'Alice'");
  const [rows] = await conn.query("SELECT JSON_EXTRACT(profile, '$.score') AS score FROM users WHERE name = 'Alice'");
  console.log(rows);
  await conn.end();
};

main().catch(e => { console.error('ERR', e); process.exitCode = 1; });
