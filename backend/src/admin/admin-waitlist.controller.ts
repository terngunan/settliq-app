import { Controller, Get, Query, Res, UseGuards } from '@nestjs/common';
import {
  ApiTags,
  ApiBearerAuth,
  ApiOperation,
  ApiQuery,
  ApiProduces,
} from '@nestjs/swagger';
import { Response } from 'express';
import { WaitlistService } from '../waitlist/waitlist.service';
import { JwtAuthGuard } from '../auth/guards/jwt.guard';
import { RolesGuard } from '../auth/guards/roles.guard';
import { Roles } from '../auth/decorators/roles.decorator';
import { MerchantRole } from '../merchants/entities/merchant.entity';

@ApiTags('admin / waitlist')
@ApiBearerAuth()
@UseGuards(JwtAuthGuard, RolesGuard)
@Roles(MerchantRole.ADMIN)
@Controller('admin/waitlist')
export class AdminWaitlistController {
  constructor(private readonly waitlistService: WaitlistService) {}

  /**
   * GET /api/v1/admin/waitlist/export
   * Downloads a CSV of waitlist members.
   * Unsubscribed members are excluded by default; pass includeUnsubscribed=true to opt in.
   */
  @Get('export')
  @ApiOperation({ summary: 'Export waitlist as CSV' })
  @ApiProduces('text/csv')
  @ApiQuery({ name: 'country', required: false, description: 'Filter by ISO country code' })
  @ApiQuery({ name: 'dateFrom', required: false, description: 'ISO date string' })
  @ApiQuery({ name: 'dateTo', required: false, description: 'ISO date string' })
  @ApiQuery({
    name: 'includeUnsubscribed',
    required: false,
    description: 'Set true to include unsubscribed members',
  })
  async exportCsv(
    @Query('country') country?: string,
    @Query('dateFrom') dateFrom?: string,
    @Query('dateTo') dateTo?: string,
    @Query('includeUnsubscribed') includeUnsubscribed?: string,
    @Res({ passthrough: true }) res?: Response,
  ): Promise<string> {
    const csv = await this.waitlistService.exportCsv({
      country,
      dateFrom,
      dateTo,
      includeUnsubscribed: includeUnsubscribed === 'true',
    });

    res.setHeader('Content-Type', 'text/csv');
    res.setHeader('Content-Disposition', 'attachment; filename="waitlist-export.csv"');

    return csv;
  }
}
