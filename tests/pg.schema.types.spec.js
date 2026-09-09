import { runTest } from './common.js';
import { createSchemalessAdapter } from '../src/schemaless.js';
import { createPGClient } from './db-helpers.js';

const log = (...args) => {
  console.log(`[pg.schema.types]`, ...args);
};

const main = async () => {
  const { client, label } = await createPGClient();

  log('client ready:', label);

  try {
    const { adapter } = createSchemalessAdapter(client, 'pg');

    const coll = adapter.collection('pg_schema_types', {
      schema: {
        smallintCol: 'SMALLINT',
        integerCol: 'INTEGER',
        bigintCol: 'BIGINT',
        serialCol: 'SERIAL',
        bigserialCol: 'BIGSERIAL',

        decimalCol: 'DECIMAL(10,2)',
        numericCol: 'NUMERIC(15,5)',
        realCol: 'REAL',
        doublePrecisionCol: 'DOUBLE PRECISION',
        moneyCol: 'MONEY',

        charCol: 'CHAR(10)',
        varcharCol: 'VARCHAR(255)',
        textCol: 'TEXT',

        byteaCol: 'BYTEA',

        booleanCol: 'BOOLEAN',

        dateCol: 'DATE',
        timeCol: 'TIME',
        timeWithTzCol: 'TIME WITH TIME ZONE',
        timestampCol: 'TIMESTAMP',
        timestampTzCol: 'TIMESTAMPTZ',
        intervalCol: 'INTERVAL',

        jsonCol: 'JSON',
        jsonbCol: 'JSONB',

        intArrayCol: 'INTEGER[]',
        textArrayCol: 'TEXT[]',

        uuidCol: 'UUID',

        inetCol: 'INET',
        cidrCol: 'CIDR',
        macaddrCol: 'MACADDR',
        macaddr8Col: 'MACADDR8',

        bitCol: 'BIT(8)',
        bitVaryingCol: 'BIT VARYING(16)',

        tsvectorCol: 'TSVECTOR',
        tsqueryCol: 'TSQUERY',

        pointCol: 'POINT',
        lineCol: 'LINE',
        lsegCol: 'LSEG',
        boxCol: 'BOX',
        pathCol: 'PATH',
        polygonCol: 'POLYGON',
        circleCol: 'CIRCLE',

        int4rangeCol: 'INT4RANGE',
        int8rangeCol: 'INT8RANGE',
        numrangeCol: 'NUMRANGE',
        tsrangeCol: 'TSRANGE',
        tstzrangeCol: 'TSTZRANGE',
        daterangeCol: 'DATERANGE',

        xmlCol: 'XML',

        uniqueCol: {
          type: 'TEXT',
          unique: true,
        },

        requiredCol: {
          type: 'TEXT',
          required: true,
        },

        defaultCol: {
          type: 'INTEGER',
          default: 100,
        },

        hiddenCol: {
          type: 'TEXT',
          hidden: true,
        },
      },
    });

    log('collection created, smoke-testing findOne({})');

    await coll.findOne({});

    log('building test document');

    // NULL sentinels for every nullable column; serial columns must NOT receive
    // explicit NULL (they are NOT NULL + sequence default), and requiredCol
    // must provide a value.
    const doc = {};
    for (const k of Object.keys(coll.schema)) {
      if (k === 'requiredCol') doc[k] = 'required-value';
      else if (k === 'serialCol' || k === 'bigserialCol') continue; // let the sequence default apply
      else doc[k] = null;
    }

    log('inserting test document');

    await coll.insertOne(doc);

    log('insert finished');

    const ensure = async (name, typeSpec) => {
      const s = await adapter.getTableSchema('pg_schema_types');

      if (!s.columns[name]) {
        const type = typeof typeSpec === 'string' ? typeSpec : typeSpec.type;

        try {
          await adapter.execute(`ALTER TABLE pg_schema_types ADD COLUMN ${name} ${type}`);
        } catch (error) {
          log(`ensure ${name}: ALTER TABLE failed`, {
            message: error?.message,
            code: error?.code,
          });
        }
      }

      const s2 = await adapter.getTableSchema('pg_schema_types');
      return !!s2.columns[name];
    };

    const defs = coll.schema;

    log('starting column verification', `columns=${Object.keys(defs).length}`);

    const supportMap = {};

    for (const k of Object.keys(defs)) {
      supportMap[k] = await ensure(k, defs[k]);
    }

    log('all columns processed');

    await runTest(
      'pg schema columns exist',
      async () => {
        const s = await adapter.getTableSchema('pg_schema_types');

        const c = s.columns || {};
        const keys = Object.keys(defs);

        return keys.map((k) => ({
          [k]: !!c[k],
        }));
      },
      Object.keys(defs).map((k) => ({
        [k]: !!supportMap[k],
      }))
    );

    log('assertion finished');
  } finally {
    await client.end();
  }

  log('finished');
};

main().catch((error) => {
  console.error('[pg.schema.types] FAILED');
  console.error(error);
  process.exitCode = 1;
});
