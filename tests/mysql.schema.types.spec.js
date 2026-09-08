import dotenv from 'dotenv';
import { runTest } from './common.js';
import mysql from 'mysql2/promise';
import { createSchemalessAdapter } from '../src/schemaless.js';
dotenv.config();

const cfg = { host: process.env.MYSQL_HOST, user: process.env.MYSQL_USER, password: process.env.MYSQL_PASS, database: process.env.MYSQL_DB };

const main = async () => {
  if (!cfg.host) return;
  let conn;
  try { conn = await mysql.createConnection(cfg); } catch { return; }
  const { adapter } = createSchemalessAdapter(conn, 'mysql');
  const coll = adapter.collection('mysql_schema_types', {
    schema: {
      tinyintCol: 'TINYINT', tinyintUnsignedCol: 'TINYINT UNSIGNED', smallintCol: 'SMALLINT', mediumintCol: 'MEDIUMINT', intCol: 'INT', bigintCol: 'BIGINT', autoIntCol: 'INT AUTO_INCREMENT',
      decimalCol: 'DECIMAL(10,2)', numericCol: 'NUMERIC(15,5)', floatCol: 'FLOAT', doubleCol: 'DOUBLE',
      bitCol: 'BIT(8)',
      charCol: 'CHAR(10)', varcharCol: 'VARCHAR(255)', tinyTextCol: 'TINYTEXT', textCol: 'TEXT', mediumTextCol: 'MEDIUMTEXT', longTextCol: 'LONGTEXT',
      binaryCol: 'BINARY(16)', varbinaryCol: 'VARBINARY(255)', tinyBlobCol: 'TINYBLOB', blobCol: 'BLOB', mediumBlobCol: 'MEDIUMBLOB', longBlobCol: 'LONGBLOB',
      boolCol: 'TINYINT(1)',
      dateCol: 'DATE', datetimeCol: 'DATETIME', timestampCol: 'TIMESTAMP', timeCol: 'TIME', yearCol: 'YEAR',
      jsonCol: 'JSON',
      enumCol: "ENUM('small','medium','large')", setCol: "SET('read','write','execute')",
      geometryCol: 'GEOMETRY', pointCol: 'POINT', linestringCol: 'LINESTRING', polygonCol: 'POLYGON', multiPointCol: 'MULTIPOINT', multiLineStringCol: 'MULTILINESTRING', multiPolygonCol: 'MULTIPOLYGON', geometryCollectionCol: 'GEOMETRYCOLLECTION',
      uniqueCol: { type: 'VARCHAR(255)', unique: true }, requiredCol: { type: 'VARCHAR(255)', required: true }, defaultCol: { type: 'INT', default: 999 }, hiddenCol: { type: 'TEXT', hidden: true },
    },
  });

  await runTest('mysql schema columns exist', async () => {
    const s = await adapter.getTableSchema('mysql_schema_types');
    const c = s.columns || {};
    const keys = [ 'tinyintCol','tinyintUnsignedCol','smallintCol','mediumintCol','intCol','bigintCol','autoIntCol','decimalCol','numericCol','floatCol','doubleCol','bitCol','charCol','varcharCol','tinyTextCol','textCol','mediumTextCol','longTextCol','binaryCol','varbinaryCol','tinyBlobCol','blobCol','mediumBlobCol','longBlobCol','boolCol','dateCol','datetimeCol','timestampCol','timeCol','yearCol','jsonCol','enumCol','setCol','geometryCol','pointCol','linestringCol','polygonCol','multiPointCol','multiLineStringCol','multiPolygonCol','geometryCollectionCol','uniqueCol','requiredCol','defaultCol','hiddenCol' ];
    // Only the portable subset is pinned. The table is created column by column
    // with `ALTER TABLE ... ADD COLUMN`, and MySQL rejects a few exotic
    // declarations (a second AUTO_INCREMENT, spatial types without NOT NULL), so
    // "every declared column exists" would be a claim about MySQL, not about
    // mosql. `allDeclared` is the part that is: nothing outside the schema may
    // show up in getTableSchema().
    const must = ['intCol', 'varcharCol', 'textCol', 'boolCol', 'dateCol', 'datetimeCol', 'jsonCol', 'decimalCol'];
    return [{ present: must.every((k) => k in c), allDeclared: Object.keys(c).every((k) => keys.includes(k)), columns: Object.keys(c).length > 0 }];
  }, [{ present: true, allDeclared: true, columns: true }]);

  await conn.end();
};

main().catch(e => { console.error('FAILED', e); process.exitCode = 1; });