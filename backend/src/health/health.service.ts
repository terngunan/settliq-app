import { Injectable } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectDataSource } from '@nestjs/typeorm';
import { DataSource } from 'typeorm';

export interface ComponentHealth {
  status: 'healthy' | 'degraded' | 'down';
  latency: number;
  message?: string;
}

export interface HealthReport {
  status: 'healthy' | 'degraded' | 'down';
  timestamp: string;
  components: {
    database: ComponentHealth;
    stellar: ComponentHealth;
  };
}

@Injectable()
export class HealthService {
  constructor(
    @InjectDataSource() private readonly dataSource: DataSource,
    private readonly config: ConfigService,
  ) {}

  async check(): Promise<HealthReport> {
    const [database, stellar] = await Promise.all([
      this.checkDatabase(),
      this.checkStellar(),
    ]);

    const statuses = [database.status, stellar.status];
    let status: HealthReport['status'] = 'healthy';
    if (statuses.includes('down')) {
      status = 'down';
    } else if (statuses.includes('degraded')) {
      status = 'degraded';
    }

    return {
      status,
      timestamp: new Date().toISOString(),
      components: { database, stellar },
    };
  }

  async checkDatabase(): Promise<ComponentHealth> {
    const start = Date.now();
    try {
      await this.dataSource.query('SELECT 1');
      return { status: 'healthy', latency: Date.now() - start };
    } catch (err) {
      return {
        status: 'down',
        latency: Date.now() - start,
        message: err instanceof Error ? err.message : 'Database unreachable',
      };
    }
  }

  async checkStellar(): Promise<ComponentHealth> {
    const start = Date.now();
    const horizonUrl =
      this.config.get<string>('STELLAR_HORIZON_URL') ?? 'https://horizon.stellar.org';
    const timeoutMs = this.config.get<number>('STELLAR_HEALTH_TIMEOUT_MS', 5000);

    try {
      const res = await fetch(`${horizonUrl}`, {
        signal: AbortSignal.timeout(timeoutMs),
        headers: { Accept: 'application/json' },
      });

      if (!res.ok) {
        return {
          status: 'degraded',
          latency: Date.now() - start,
          message: `Horizon responded with status ${res.status}`,
        };
      }

      return { status: 'healthy', latency: Date.now() - start };
    } catch (err) {
      return {
        status: 'down',
        latency: Date.now() - start,
        message: err instanceof Error ? err.message : 'Stellar Horizon unreachable',
      };
    }
  }
}
