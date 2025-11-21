import { escape, jsonPath, validate, isObject, is$ } from '../src/index.js';
import { runStringTest, runTest } from './common.js';

const dbs = ['sqlite','pg','mysql'];

// isObject / is$
runStringTest('isObject true', () => String(isObject({ a: 1 })), 'true');
runStringTest('isObject false null', () => String(isObject(null)), 'false');
runStringTest('isObject false array', () => String(isObject([1,2])), 'false');
runStringTest('is$ true', () => String(is$(' $'.trim() + 'x'.padStart(1,'$').slice(0))), 'true');
runStringTest('is$ false', () => String(is$('x')), 'false');

// validate.col and alias
for (const db of dbs) {
  runStringTest(`validate.col ${db}`, () => validate.col('users', db), 'users');
  runStringTest(`validate.alias ${db}`, () => validate.alias('user-name'), 'user_name');
}

// escape primitives
for (const db of dbs) {
  runStringTest(`escape number ${db}`, () => escape(42, db), '42');
  runStringTest(`escape boolean true ${db}`, () => escape(true, db), db==='pg' ? 'TRUE' : '1');
  runStringTest(`escape boolean false ${db}`, () => escape(false, db), db==='pg' ? 'FALSE' : '0');
  runStringTest(`escape string ${db}`, () => escape("O'Hara", db), "'O''Hara'");
  runStringTest(`escape date ${db}`, () => { const d = new Date('2024-01-01T00:00:00.000Z'); return escape(d, db); }, db==='pg' ? "'2024-01-01T00:00:00.000Z'::timestamp" : db==='mysql' ? "'2024-01-01 00:00:00'" : "'2024-01-01T00:00:00.000Z'" );
  runStringTest(`escape object ${db}`, () => escape({ a: 1 }, db), db==='pg' ? "'{" + '"a"' + ":1}'::jsonb" : "'{" + '"a"' + ":1}'");
}

// jsonPath for field and nested
for (const db of dbs) {
  runStringTest(`jsonPath field ${db}`, () => jsonPath('profile', db), 'profile');
  runStringTest(`jsonPath nested ${db}`, () => jsonPath('profile.score', db), db==='pg' ? "(profile::jsonb #>> '{score}')" : db==='mysql' ? "JSON_UNQUOTE(JSON_EXTRACT(profile, '$.score'))" : "json_extract(profile, '$.score')" );
}