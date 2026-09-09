import { runTest } from './common.js';
import { createSchemalessAdapter } from '../src/schemaless.js';
import { createMySQLConn } from './db-helpers.js';

const main = async () => {
  const myCtx = await createMySQLConn();
  if (!myCtx) {
    console.log('[mysql.schema.types] database unavailable, skipping');
    return;
  }
  const { conn, stop } = myCtx;

  const { adapter } = createSchemalessAdapter(conn, 'mysql');
  // Note: only _id carries AUTO_INCREMENT — MySQL permits one auto-increment
  // column per table, and `_id INT AUTO_INCREMENT PRIMARY KEY` is always created.
  const coll = adapter.collection('mysql_schema_types', {
    schema: {
      tinyintCol: 'TINYINT', tinyintUnsignedCol: 'TINYINT UNSIGNED', smallintCol: 'SMALLINT', mediumintCol: 'MEDIUMINT', intCol: 'INT', bigintCol: 'BIGINT',
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

  const keys = ['tinyintCol','tinyintUnsignedCol','smallintCol','mediumintCol','intCol','bigintCol','decimalCol','numericCol','floatCol','doubleCol','bitCol','charCol','varcharCol','tinyTextCol','textCol','mediumTextCol','longTextCol','binaryCol','varbinaryCol','tinyBlobCol','blobCol','mediumBlobCol','longBlobCol','boolCol','dateCol','datetimeCol','timestampCol','timeCol','yearCol','jsonCol','enumCol','setCol','geometryCol','pointCol','linestringCol','polygonCol','multiPointCol','multiLineStringCol','multiPolygonCol','geometryCollectionCol','uniqueCol','requiredCol','defaultCol','hiddenCol'];

  try {
    // Force table + column creation, then insert a row so the schema is real.
    await coll.insertOne({ requiredCol: 'x' });

    await runTest('mysql schema columns exist', async () => {
      const s = await adapter.getTableSchema('mysql_schema_types');
      const c = s.columns || {};
      return keys.map(k => ({ [k]: k in c }));
    }, keys.map(k => ({ [k]: true })));
  } finally {
    await conn.end().catch(() => {});
    await stop().catch?.(() => {});
  }
};

await main().catch(e => {
  console.error('[mysql.schema.types] FAILED');
  console.error(e);
  process.exitCode = 1;
});
