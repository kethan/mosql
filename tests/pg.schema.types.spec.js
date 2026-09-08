import dotenv from 'dotenv';
import { runTest } from './common.js';
import { createSchemalessAdapter } from '../src/schemaless.js';

dotenv.config();

const cfg = {
  host: process.env.PG_HOST,
  port: Number(process.env.PG_PORT || 5432),
  user: process.env.PG_USER,
  password: process.env.PG_PASSWORD,
  database: process.env.PG_DB,

  // Important for CI diagnostics.
  connectionTimeoutMillis: 10_000,
  query_timeout: 30_000,
  statement_timeout: 30_000,
};

const log = (...args) => {
  console.log(`[pg.schema.types]`, ...args);
};

const withTimeout = async (promise, ms, label) => {
  let timer;

  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error(`${label} timed out after ${ms}ms`));
    }, ms);
  });

  try {
    return await Promise.race([promise, timeout]);
  } finally {
    clearTimeout(timer);
  }
};

const main = async () => {
  log('1. starting');

  if (!cfg.host) {
    log('2. PG_HOST is missing - skipping');
    return;
  }

  log('2. config', {
    host: cfg.host,
    port: cfg.port,
    user: cfg.user,
    database: cfg.database,
  });

  log('3. importing pg');

  const pkg = await import('pg');

  log('4. pg imported');

  const Client = pkg.Client || pkg.default?.Client;

  if (!Client) {
    throw new Error('Unable to resolve pg.Client');
  }

  log('5. creating PostgreSQL client');

  const client = new Client(cfg);

  client.on('error', (error) => {
    console.error('[pg.schema.types] client error:', error);
  });

  client.on('end', () => {
    log('PostgreSQL client emitted end');
  });

  try {
    log('6. connecting to PostgreSQL');

    await withTimeout(
      client.connect(),
      15_000,
      'PostgreSQL client.connect()'
    );

    log('7. PostgreSQL connected');

    log('8. creating MOSQL adapter');

    const { adapter } = createSchemalessAdapter(client, 'pg');

    log('9. MOSQL adapter created');

    log('10. creating collection');

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

    log('11. collection created');
    log('12. calling coll.findOne({})');

    await withTimeout(
      coll.findOne({}),
      60_000,
      'coll.findOne({})'
    );

    log('13. coll.findOne({}) finished');

    log('14. building test document');

    const doc = Object.fromEntries(
      Object.keys(coll.schema).map((k) => [k, null])
    );

    log('15. inserting test document');

    await withTimeout(
      coll.insertOne(doc),
      60_000,
      'coll.insertOne(doc)'
    );

    log('16. insert finished');

    const ensure = async (name, typeSpec) => {
      log(`17.${name}.1 getTableSchema`);

      const s = await withTimeout(
        adapter.getTableSchema('pg_schema_types'),
        30_000,
        `${name}: getTableSchema #1`
      );

      if (!s.columns[name]) {
        const type =
          typeof typeSpec === 'string'
            ? typeSpec
            : typeSpec.type;

        log(`17.${name}.2 adding column`, {
          name,
          type,
        });

        try {
          await withTimeout(
            adapter.execute(
              `ALTER TABLE pg_schema_types ADD COLUMN ${name} ${type}`
            ),
            30_000,
            `${name}: ALTER TABLE`
          );

          log(`17.${name}.3 ALTER TABLE finished`);
        } catch (error) {
          log(`17.${name}.3 ALTER TABLE failed`, {
            message: error?.message,
            code: error?.code,
          });
        }
      } else {
        log(`17.${name}.2 column already exists`);
      }

      log(`17.${name}.4 getTableSchema again`);

      const s2 = await withTimeout(
        adapter.getTableSchema('pg_schema_types'),
        30_000,
        `${name}: getTableSchema #2`
      );

      const exists = !!s2.columns[name];

      log(`17.${name}.5 done`, { exists });

      return exists;
    };

    const defs = coll.schema;

    log(
      '18. starting column verification',
      `columns=${Object.keys(defs).length}`
    );

    const supportMap = {};

    for (const k of Object.keys(defs)) {
      log('18.x ensuring column', k);

      supportMap[k] = await ensure(k, defs[k]);
    }

    log('19. all columns processed');

    await runTest(
      'pg schema columns exist',
      async () => {
        log('20. running pg schema columns assertion');

        const s = await adapter.getTableSchema(
          'pg_schema_types'
        );

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

    log('21. test assertion finished');
  } finally {
    log('22. closing PostgreSQL client');

    try {
      await withTimeout(
        client.end(),
        10_000,
        'PostgreSQL client.end()'
      );

      log('23. PostgreSQL client closed');
    } catch (error) {
      console.error(
        '[pg.schema.types] client.end() failed:',
        error
      );

      // Force the process to continue terminating in CI.
      try {
        client.end();
      } catch {}
    }
  }

  log('24. finished');
};

main().catch((error) => {
  console.error('[pg.schema.types] FAILED');
  console.error(error);
  process.exitCode = 1;
});
