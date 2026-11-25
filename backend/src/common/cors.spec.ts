import { Controller, INestApplication, Post } from '@nestjs/common';
import { Test } from '@nestjs/testing';
import request = require('supertest');
import { configureCors } from './cors';

@Controller('auth')
class TestAuthController {
  @Post('login')
  login() {
    return {};
  }
}

describe('API CORS (e2e)', () => {
  const merchantOrigin = 'https://merchant.example';
  let app: INestApplication;

  beforeEach(async () => {
    const env = { ...process.env };
    delete env.CORS_ALLOWED_ORIGINS;
    delete env.CORS_ORIGIN;
    jest.replaceProperty(process, 'env', env);

    const module = await Test.createTestingModule({
      controllers: [TestAuthController],
    }).compile();
    app = module.createNestApplication();
    app.setGlobalPrefix('api/v1');
  });

  afterEach(async () => {
    jest.restoreAllMocks();
    await app.close();
  });

  async function preflight(origin: string) {
    configureCors(app);
    await app.init();

    return request(app.getHttpServer())
      .options('/api/v1/auth/login')
      .set('Origin', origin)
      .set('Access-Control-Request-Method', 'POST')
      .set('Access-Control-Request-Headers', 'content-type')
      .expect(204);
  }

  it('allows configured origins on preflight and API responses', async () => {
    process.env.CORS_ORIGIN = `https://dashboard.example, ${merchantOrigin} `;
    const response = await preflight(merchantOrigin);

    expect(response.headers['access-control-allow-origin']).toBe(merchantOrigin);
    expect(response.headers['access-control-allow-credentials']).toBe('true');
    expect(response.headers['access-control-allow-methods']).toContain('POST');
    expect(response.headers['access-control-allow-headers']).toBe('content-type');

    await request(app.getHttpServer())
      .post('/api/v1/auth/login')
      .set('Origin', merchantOrigin)
      .expect('Access-Control-Allow-Origin', merchantOrigin)
      .expect(201);
  });

  it('honors the existing allowlist when both variables are set', async () => {
    process.env.CORS_ALLOWED_ORIGINS = 'https://dashboard.example';
    process.env.CORS_ORIGIN = merchantOrigin;
    const response = await preflight(merchantOrigin);

    expect(response.headers['access-control-allow-origin']).toBeUndefined();
  });

  it('allows any origin without credentials when no list is configured', async () => {
    const response = await preflight(merchantOrigin);

    expect(response.headers['access-control-allow-origin']).toBe('*');
    expect(response.headers['access-control-allow-credentials']).toBeUndefined();
  });
});
