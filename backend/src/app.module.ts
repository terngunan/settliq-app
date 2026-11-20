import { Module, ValidationPipe } from '@nestjs/common';
import { ConfigModule, ConfigService } from '@nestjs/config';
import { TypeOrmModule } from '@nestjs/typeorm';
import { APP_GUARD, APP_PIPE } from '@nestjs/core';
import { ThrottlerModule, ThrottlerGuard } from '@nestjs/throttler';
import { AuthModule } from './auth/auth.module';
import { WaitlistModule } from './waitlist/waitlist.module';
import { AdminModule } from './admin/admin.module';
import { HealthModule } from './health/health.module';
import { Merchant } from './merchants/entities/merchant.entity';
import { WaitlistEntry } from './waitlist/entities/waitlist.entity';
import { RefreshToken } from './auth/refresh-token.entity';

@Module({
  imports: [
    ConfigModule.forRoot({ isGlobal: true }),

    // ── Rate limiting (#37) ────────────────────────────────────────────────
    // Defaults (overridable via THROTTLE_TTL / THROTTLE_LIMIT env vars).
    // Auth routes override these defaults with stricter per-route decorators.
    ThrottlerModule.forRootAsync({
      imports: [ConfigModule],
      useFactory: (config: ConfigService) => [
        {
          ttl: config.get<number>('THROTTLE_TTL', 60000),
          limit: config.get<number>('THROTTLE_LIMIT', 60),
        },
      ],
      inject: [ConfigService],
    }),

    // ── Database (#38) ─────────────────────────────────────────────────────
    TypeOrmModule.forRootAsync({
      imports: [ConfigModule],
      useFactory: (config: ConfigService) => {
        const isProd = config.get<string>('NODE_ENV') === 'production';
        let ssl: boolean | { rejectUnauthorized: boolean; ca?: string } = false;

        if (isProd) {
          // Enforce certificate verification in production.
          // Provide DATABASE_CA with the PEM-encoded CA certificate when
          // connecting to a managed cloud PostgreSQL instance (e.g. AWS RDS,
          // Supabase, Railway) that uses a private CA.
          const ca = config.get<string>('DATABASE_CA');
          ssl = { rejectUnauthorized: true, ...(ca ? { ca } : {}) };
        }

        return {
          type: 'postgres',
          url: config.get<string>('DATABASE_URL'),
          entities: [Merchant, WaitlistEntry],
          // Schema sync is opt-in only (#127). Never derive this from
          // NODE_ENV: staging/preview/test/unset must rely on migrations so
          // TypeORM cannot silently drop columns on boot.
          synchronize: config.get<string>('DB_SYNCHRONIZE') === 'true',
          ssl,
        };
      },
      inject: [ConfigService],
    }),

    AuthModule,
    WaitlistModule,
    AdminModule,
    HealthModule,
  ],
  providers: [
    // Apply rate limiting globally to every route (#37).
    {
      provide: APP_GUARD,
      useClass: ThrottlerGuard,
    },
    {
      provide: APP_PIPE,
      useValue: new ValidationPipe({ whitelist: true, transform: true }),
    },
  ],
})
export class AppModule {}
