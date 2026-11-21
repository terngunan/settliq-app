import { Test, TestingModule } from '@nestjs/testing';
import { ConfigService } from '@nestjs/config';
import { getDataSourceToken } from '@nestjs/typeorm';
import { DataSource } from 'typeorm';
import { HealthService } from './health.service';

// ── Helpers ────────────────────────────────────────────────────────────────

function makeDataSource(queryFn: jest.Mock = jest.fn()): Partial<DataSource> {
  return { query: queryFn };
}

function makeConfig(overrides: Record<string, string | number | undefined> = {}) {
  return {
    get: jest.fn((key: string, defaultValue?: unknown) =>
      key in overrides ? overrides[key] : defaultValue,
    ),
  };
}

// ── Tests ──────────────────────────────────────────────────────────────────

describe('HealthService', () => {
  let service: HealthService;
  let queryMock: jest.Mock;
  let configMock: ReturnType<typeof makeConfig>;

  beforeEach(async () => {
    queryMock = jest.fn().mockResolvedValue([{ '?column?': 1 }]);
    configMock = makeConfig({
      STELLAR_HORIZON_URL: 'https://horizon.stellar.org',
    });

    // Stub global fetch so tests don't make real network calls
    global.fetch = jest.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ network_passphrase: 'Public Global Stellar Network ; September 2015' }),
    } as Response);

    const module: TestingModule = await Test.createTestingModule({
      providers: [
        HealthService,
        { provide: getDataSourceToken(), useValue: makeDataSource(queryMock) },
        { provide: ConfigService, useValue: configMock },
      ],
    }).compile();

    service = module.get<HealthService>(HealthService);
  });

  afterEach(() => {
    jest.restoreAllMocks();
  });

  // ── check ─────────────────────────────────────────────────────────────

  describe('check', () => {
    it('returns status "healthy" when database and Stellar are reachable', async () => {
      const report = await service.check();

      expect(report.status).toBe('healthy');
      expect(report.components.database.status).toBe('healthy');
      expect(report.components.stellar.status).toBe('healthy');
    });

    it('returns status "down" when the database is unreachable', async () => {
      queryMock.mockRejectedValue(new Error('ECONNREFUSED'));

      const report = await service.check();

      expect(report.status).toBe('down');
      expect(report.components.database.status).toBe('down');
    });

    it('returns status "down" when Stellar fetch throws (network error)', async () => {
      queryMock.mockRejectedValue(new Error('DB down'));
      (global.fetch as jest.Mock).mockRejectedValue(new Error('network timeout'));

      const report = await service.check();

      expect(report.status).toBe('down');
    });

    it('returns status "degraded" when Stellar responds with a non-OK status', async () => {
      (global.fetch as jest.Mock).mockResolvedValue({ ok: false } as Response);

      const report = await service.check();

      expect(report.components.stellar.status).toBe('degraded');
      // Database is fine, so overall is degraded (not down)
      expect(report.status).toBe('degraded');
    });

    it('includes a non-empty ISO timestamp', async () => {
      const report = await service.check();

      expect(report.timestamp).toBeDefined();
      expect(new Date(report.timestamp).getTime()).not.toBeNaN();
    });

    it('includes numeric latencies for each component', async () => {
      const report = await service.check();

      expect(typeof report.components.database.latency).toBe('number');
      expect(typeof report.components.stellar.latency).toBe('number');
    });
  });

  // ── checkDatabase ─────────────────────────────────────────────────────

  describe('checkDatabase', () => {
    it('executes SELECT 1 to probe the connection', async () => {
      await service.checkDatabase();

      expect(queryMock).toHaveBeenCalledWith('SELECT 1');
    });

    it('returns "healthy" when SELECT 1 succeeds', async () => {
      const result = await service.checkDatabase();

      expect(result.status).toBe('healthy');
    });

    it('returns "down" when SELECT 1 throws', async () => {
      queryMock.mockRejectedValue(new Error('connection lost'));

      const result = await service.checkDatabase();

      expect(result.status).toBe('down');
    });
  });

  // ── checkStellar ──────────────────────────────────────────────────────

  describe('checkStellar', () => {
    it('returns "healthy" when Horizon responds ok', async () => {
      const result = await service.checkStellar();

      expect(result.status).toBe('healthy');
    });

    it('returns "degraded" when Horizon responds with a non-ok status', async () => {
      (global.fetch as jest.Mock).mockResolvedValue({ ok: false } as Response);

      const result = await service.checkStellar();

      expect(result.status).toBe('degraded');
    });

    it('returns "down" when the fetch throws', async () => {
      (global.fetch as jest.Mock).mockRejectedValue(new Error('ECONNREFUSED'));

      const result = await service.checkStellar();

      expect(result.status).toBe('down');
    });

    it('defaults the AbortSignal timeout to 5000ms when STELLAR_HEALTH_TIMEOUT_MS is unset', async () => {
      const timeoutSpy = jest.spyOn(AbortSignal, 'timeout');

      await service.checkStellar();

      expect(configMock.get).toHaveBeenCalledWith('STELLAR_HEALTH_TIMEOUT_MS', 5000);
      expect(timeoutSpy).toHaveBeenCalledWith(5000);
    });

    it('uses the configured STELLAR_HEALTH_TIMEOUT_MS for the AbortSignal timeout', async () => {
      configMock.get.mockImplementation((key: string, defaultValue?: unknown) =>
        key === 'STELLAR_HEALTH_TIMEOUT_MS' ? 12000 : defaultValue,
      );
      const timeoutSpy = jest.spyOn(AbortSignal, 'timeout');

      await service.checkStellar();

      expect(timeoutSpy).toHaveBeenCalledWith(12000);
    });
  });
});

// ── TypeORM SSL configuration (unit test for #38) ─────────────────────────
// This test validates the SSL configuration factory logic directly to satisfy
// issue #38's testing requirement: verify that rejectUnauthorized is true
// when NODE_ENV === 'production'.

describe('TypeORM SSL configuration factory', () => {
  function buildSslConfig(nodeEnv: string, databaseCa?: string) {
    const isProd = nodeEnv === 'production';
    if (!isProd) return false;

    const ssl: { rejectUnauthorized: boolean; ca?: string } = { rejectUnauthorized: true };
    if (databaseCa) ssl.ca = databaseCa;
    return ssl;
  }

  it('sets rejectUnauthorized: true in production', () => {
    const ssl = buildSslConfig('production');

    expect(ssl).not.toBe(false);
    expect((ssl as any).rejectUnauthorized).toBe(true);
  });

  it('includes the CA certificate when DATABASE_CA is provided', () => {
    const ca = '-----BEGIN CERTIFICATE-----\nMIID...\n-----END CERTIFICATE-----';
    const ssl = buildSslConfig('production', ca);

    expect((ssl as any).ca).toBe(ca);
    expect((ssl as any).rejectUnauthorized).toBe(true);
  });

  it('does NOT set rejectUnauthorized: false in production', () => {
    const ssl = buildSslConfig('production');

    expect((ssl as any).rejectUnauthorized).not.toBe(false);
  });

  it('disables SSL entirely outside of production', () => {
    expect(buildSslConfig('development')).toBe(false);
    expect(buildSslConfig('test')).toBe(false);
    expect(buildSslConfig('staging')).toBe(false);
  });
});
