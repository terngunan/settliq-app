import { INestApplication } from '@nestjs/common';

export function configureCors(app: INestApplication): void {
  const allowedOrigins = (
    process.env.CORS_ALLOWED_ORIGINS ?? process.env.CORS_ORIGIN
  )
    ?.split(',')
    .map((origin) => origin.trim())
    .filter((origin) => origin.length > 0);

  const hasAllowlist = Boolean(allowedOrigins?.length);
  app.enableCors({
    origin: hasAllowlist ? allowedOrigins : '*',
    // Browsers reject credentialed responses that use a wildcard origin.
    credentials: hasAllowlist,
  });
}
