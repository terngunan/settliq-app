import { Controller, Get, HttpStatus, Res } from '@nestjs/common';
import { ApiTags, ApiOperation, ApiResponse } from '@nestjs/swagger';
import { Response } from 'express';
import { HealthService, HealthReport } from './health.service';

@ApiTags('health')
@Controller('health')
export class HealthController {
  constructor(private readonly healthService: HealthService) {}

  @Get()
  @ApiOperation({ summary: 'Liveness/readiness probe' })
  @ApiResponse({ status: 200, description: 'Service is up' })
  rootHealth(): { status: string } {
    return { status: 'ok' };
  }

  @Get('admin')
  @ApiOperation({ summary: 'Detailed health report' })
  @ApiResponse({ status: 200, description: 'All components healthy' })
  @ApiResponse({ status: 503, description: 'One or more components degraded or down' })
  async adminHealth(@Res() res: Response): Promise<void> {
    const report: HealthReport = await this.healthService.check();
    const statusCode =
      report.status === 'healthy'
        ? HttpStatus.OK
        : HttpStatus.SERVICE_UNAVAILABLE;
    res.status(statusCode).json(report);
  }
}
