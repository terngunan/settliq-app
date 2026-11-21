import { NestFactory, Reflector } from '@nestjs/core';
import { ClassSerializerInterceptor } from '@nestjs/common';
import { DocumentBuilder, SwaggerModule } from '@nestjs/swagger';
import { AppModule } from './app.module';
import { AllExceptionsFilter } from './common/filters/all-exceptions.filter';

async function bootstrap() {
  const app = await NestFactory.create(AppModule);

  // Issue #47: Register a global exception filter to sanitize unhandled errors
  // and prevent internal stack traces or database details from reaching clients.
  app.useGlobalFilters(new AllExceptionsFilter());

  app.setGlobalPrefix('api/v1');
  app.useGlobalInterceptors(new ClassSerializerInterceptor(app.get(Reflector)));

  // Issue #48: Enable CORS so browser clients on different origins (e.g. the
  // merchant dashboard or customer waitlist landing page) can reach the API.
  // Allowed origins are configurable via CORS_ALLOWED_ORIGINS (comma-separated).
  // Issue #207: Per the Fetch/CORS spec, `Access-Control-Allow-Origin: *` must
  // never be combined with `Access-Control-Allow-Credentials: true`. Only enable
  // credentials when explicit non-wildcard origins are configured; otherwise
  // fall back to a wildcard origin without credentials.
  const allowedOrigins = process.env.CORS_ALLOWED_ORIGINS?.split(',')
    .map((origin) => origin.trim())
    .filter((origin) => origin.length > 0);

  if (allowedOrigins && allowedOrigins.length > 0) {
    app.enableCors({
      origin: allowedOrigins,
      credentials: true,
    });
  } else {
    app.enableCors({
      origin: '*',
      credentials: false,
    });
  }

  const config = new DocumentBuilder()
    .setTitle('Settliq API')
    .setDescription('Merchant registration, API key auth, waitlist export & GDPR')
    .setVersion('1.0')
    .addBearerAuth()
    .addApiKey({ type: 'apiKey', in: 'header', name: 'x-api-key' }, 'x-api-key')
    .build();

  SwaggerModule.setup('docs', app, SwaggerModule.createDocument(app, config));

  await app.listen(process.env.PORT ?? 3000);
}

bootstrap();
