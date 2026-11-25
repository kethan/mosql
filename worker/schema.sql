CREATE TABLE IF NOT EXISTS "user" (
  "id" TEXT PRIMARY KEY,
  "name" TEXT NOT NULL,
  "email" TEXT NOT NULL UNIQUE,
  "emailVerified" BOOLEAN NOT NULL DEFAULT FALSE,
  "image" TEXT,
  "username" TEXT UNIQUE,
  "displayUsername" TEXT,
  "isAnonymous" BOOLEAN NOT NULL DEFAULT FALSE,
  "twoFactorEnabled" BOOLEAN NOT NULL DEFAULT FALSE,
  "role" TEXT NOT NULL DEFAULT 'user',
  "banned" BOOLEAN NOT NULL DEFAULT FALSE,
  "banReason" TEXT,
  "banExpires" TEXT,
  "createdAt" TEXT NOT NULL,
  "updatedAt" TEXT NOT NULL,
  "data" TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS "session" (
  "id" TEXT PRIMARY KEY,
  "userId" TEXT NOT NULL,
  "token" TEXT NOT NULL UNIQUE,
  "expiresAt" TEXT,
  "ipAddress" TEXT,
  "userAgent" TEXT,
  "impersonatedBy" TEXT,
  "createdAt" TEXT NOT NULL,
  "updatedAt" TEXT NOT NULL,
  "data" TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS "account" (
  "id" TEXT PRIMARY KEY,
  "userId" TEXT NOT NULL,
  "accountId" TEXT NOT NULL,
  "providerId" TEXT NOT NULL,
  "accessToken" TEXT,
  "refreshToken" TEXT,
  "idToken" TEXT,
  "accessTokenExpiresAt" TEXT,
  "refreshTokenExpiresAt" TEXT,
  "scope" TEXT,
  "password" TEXT,
  "createdAt" TEXT NOT NULL,
  "updatedAt" TEXT NOT NULL,
  "data" TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS "verification" (
  "id" TEXT PRIMARY KEY,
  "identifier" TEXT NOT NULL,
  "value" TEXT NOT NULL,
  "expiresAt" TEXT NOT NULL,
  "createdAt" TEXT NOT NULL,
  "updatedAt" TEXT NOT NULL,
  "data" TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS "twoFactor" (
  "id" TEXT PRIMARY KEY,
  "userId" TEXT NOT NULL UNIQUE,
  "secret" TEXT NOT NULL,
  "backupCodes" TEXT NOT NULL,
  "data" TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS "session_userId" ON "session" ("userId");
CREATE INDEX IF NOT EXISTS "account_userId" ON "account" ("userId");
CREATE INDEX IF NOT EXISTS "account_provider" ON "account" ("providerId", "accountId");
CREATE INDEX IF NOT EXISTS "verification_identifier" ON "verification" ("identifier");
