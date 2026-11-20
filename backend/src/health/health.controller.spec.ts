import { Test, TestingModule } from '@nestjs/testing';
import { HttpStatus } from '@nestjs/common';
import { HealthController } from './health.controller';
import { HealthService, HealthReport } from './health.service';

// ── Helpers ────────────────────────────────────────────────────────────────

function makeReport(status: HealthReport['status'] = 'healthy'): HealthReport {
  return {
    status,
    timestamp: new Date().toISOString(),
    components: {
      database: { status: 'healthy', latency: 5 },
      stellar: { status: 'healthy', latency: 120 },
    },
  };
}

function makeResponse() {
  const json = jest.fn();
  const status = jest.fn().mockReturnValue({ json });
  return { status, json, _json: json };
}

// ── Tests ──────────────────────────────────────────────────────────────────

describe('HealthController', () => {
  let controller: HealthController;
  let healthService: jest.Mocked<Pick<HealthService, 'check'>>;

  beforeEach(async () => {
    healthService = { check: jest.fn() };

    const module: TestingModule = await Test.createTestingModule({
      controllers: [HealthController],
      providers: [{ provide: HealthService, useValue: healthService }],
    }).compile();

    controller = module.get<HealthController>(HealthController);
  });

  // ── GET /health ───────────────────────────────────────────────────────

  describe('rootHealth', () => {
    it('returns { status: "ok" } for liveness/readiness probes', () => {
      expect(controller.rootHealth()).toEqual({ status: 'ok' });
    });

    it('does not call the health service', () => {
      controller.rootHealth();
      expect(healthService.check).not.toHaveBeenCalled();
    });
  });

  // ── GET /health/admin ─────────────────────────────────────────────────

  describe('adminHealth', () => {
    it('responds 200 with the health report when all components are healthy', async () => {
      const report = makeReport('healthy');
      healthService.check.mockResolvedValue(report);
      const res = makeResponse();

      await controller.adminHealth(res as any);

      expect(res.status).toHaveBeenCalledWith(HttpStatus.OK);
      expect(res._json).toHaveBeenCalledWith(report);
    });

    it('responds 503 when the overall status is "down"', async () => {
      const report = makeReport('down');
      report.components.database = { status: 'down', latency: 0 };
      healthService.check.mockResolvedValue(report);
      const res = makeResponse();

      await controller.adminHealth(res as any);

      expect(res.status).toHaveBeenCalledWith(HttpStatus.SERVICE_UNAVAILABLE);
      expect(res._json).toHaveBeenCalledWith(report);
    });

    it('responds 503 when the overall status is "degraded"', async () => {
      const report = makeReport('degraded');
      healthService.check.mockResolvedValue(report);
      const res = makeResponse();

      await controller.adminHealth(res as any);

      expect(res.status).toHaveBeenCalledWith(HttpStatus.SERVICE_UNAVAILABLE);
    });

    it('includes a timestamp in the response body', async () => {
      const report = makeReport('healthy');
      healthService.check.mockResolvedValue(report);
      const res = makeResponse();

      await controller.adminHealth(res as any);

      const body = res._json.mock.calls[0][0] as HealthReport;
      expect(body.timestamp).toBeDefined();
      expect(new Date(body.timestamp).getTime()).not.toBeNaN();
    });

    it('includes component health details in the response body', async () => {
      const report = makeReport('healthy');
      healthService.check.mockResolvedValue(report);
      const res = makeResponse();

      await controller.adminHealth(res as any);

      const body = res._json.mock.calls[0][0] as HealthReport;
      expect(body.components.database).toBeDefined();
      expect(body.components.stellar).toBeDefined();
      expect(typeof body.components.database.latency).toBe('number');
      expect(typeof body.components.stellar.latency).toBe('number');
    });

    it('conforms to the shape expected by monitoring/statuspage/worker.js', async () => {
      const report = makeReport('healthy');
      healthService.check.mockResolvedValue(report);
      const res = makeResponse();

      await controller.adminHealth(res as any);

      const body = res._json.mock.calls[0][0] as HealthReport;
      // worker.js parses: body.status, body.timestamp, body.components.*
      expect(['healthy', 'degraded', 'down']).toContain(body.status);
      expect(body.components).toHaveProperty('database');
      expect(body.components).toHaveProperty('stellar');
    });
  });
});
