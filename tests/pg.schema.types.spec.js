import { loadEnv } from '../src/env.js';
import { runTest, pgConfig, isConfigured, skipMessage, connectSkip, shapeOf, nullableColumns } from './common.js';
import { createSchemalessAdapter } from '../src/schemaless.js';

await loadEnv();

// The timeouts are what this file adds on top of the shared connection config: a
// hung connection should show up as an error, not as a hung CI job.
const cfg = {
  ...pgConfig(),
  connectionTimeoutMillis: 10_000,
  query_timeout: 30_000,
  statement_timeout: 30_000,
};
const label = 'pg.schema.types';

const log = (...args) => {
  console.log(`[pg.schema.types]`, ...args);
};

const withTimeout = async (promise, ms, what) => {
  let timer;

  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error(`${what} timed out after ${ms}ms`));
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

  if (!isConfigured(cfg)) {
    log(`2. ${skipMessage(label, 'pg', cfg)}`);
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

    const shape = (k) => shapeOf(coll.schema[k]);
    const required = Object.keys(coll.schema).filter((k) => shape(k).required);
    const withDefault = Object.keys(coll.schema).filter((k) => shape(k).default !== undefined);

    // NULL for every column that accepts it - `nullableColumns` is what decides
    // which ones those are, and it is asserted on its own in util.spec.js.
    const nullableKeys = nullableColumns(coll.schema);
    const doc = Object.fromEntries(nullableKeys.map((k) => [k, null]));

    // pg_schema_types outlives the process (indexes are only synced in CI), so
    // "the first row" is whatever a previous run left behind. The marker pins the
    // assertions below to the row this run inserted.
    const marker = `pg_nullability_probe_${Date.now()}`;
    if ('requiredCol' in coll.schema) doc.requiredCol = 'probe';
    if ('uniqueCol' in coll.schema) doc.uniqueCol = marker;
    const probeWhere = 'uniqueCol' in coll.schema ? { uniqueCol: marker } : {};

    log('15. inserting test document', {
      columns: Object.keys(doc).length,
      skipped: Object.keys(coll.schema).filter((k) => !nullableKeys.includes(k)),
    });

    await withTimeout(
      coll.insertOne(doc),
      60_000,
      'coll.insertOne(doc)'
    );

    log('16. insert finished');

    await runTest('pg schema defaults and NOT NULL', async () => {
      // a column with a DEFAULT keeps it when the document omits the key
      const row = await coll.findOne(probeWhere);
      const defaults = {};
      for (const k of withDefault) defaults[k] = row[k];

      // a NOT NULL column rejects NULL - and it has to be that constraint
      // complaining, naming that column: the row also carries a unique value, so a
      // loose /violates/ test would accept a duplicate-key error as proof.
      let rejected = false;
      try {
        await coll.insertOne({ ...doc, [required[0]]: null });
      } catch (e) {
        const msg = String(e?.message || e).toLowerCase();
        rejected = /not-null/i.test(msg) && msg.includes(`"${required[0].toLowerCase()}"`);
      }

      return [{ defaults, rejected }];
    }, [{ defaults: { defaultCol: 100 }, rejected: true }]);

    // Postgres folds unquoted identifiers to lower case and information_schema
    // reports them that way, while this schema is written camelCase - so a plain
    // `columns[name]` lookup never matches and every run re-issues its ALTERs.
    const fold = (source) => Object.fromEntries(
      Object.entries(source.columns || {}).map(([name, type]) => [name.toLowerCase(), type])
    );

    const ensure = async (name, typeSpec) => {
      log(`17.${name}.1 getTableSchema`);

      const s = await withTimeout(
        adapter.getTableSchema('pg_schema_types'),
        30_000,
        `${name}: getTableSchema #1`
      );

      if (!fold(s)[name.toLowerCase()]) {
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

      const exists = !!fold(s2)[name.toLowerCase()];

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

        const c = fold(s);
        const keys = Object.keys(defs);

        return keys.map((k) => ({
          [k]: !!c[k.toLowerCase()],
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
  // A configured but unreachable server is an environment problem, not a code
  // failure - report it as a skip so `npm test` stays meaningful offline.
  if (/ECONNREFUSED|ETIMEDOUT|ENOTFOUND|EHOSTUNREACH|getaddrinfo|authentication|pg_hba/i.test(String(error?.message || error))) {
    console.log(connectSkip(label, cfg, error));
    return;
  }

  console.error('[pg.schema.types] FAILED');
  console.error(error);
  process.exitCode = 1;
});
