import { Controller, Post, Get, Delete, Body, Param, Query, Res, HttpCode, HttpStatus, BadRequestException } from '@nestjs/common';
import {
  ApiTags,
  ApiOperation,
  ApiOkResponse,
  ApiBadRequestResponse,
  ApiConflictResponse,
  ApiUnauthorizedResponse,
  ApiQuery,
} from '@nestjs/swagger';
import { Response } from 'express';
import { WaitlistService } from './waitlist.service';
import { JoinWaitlistDto } from './dto/join-waitlist.dto';

@ApiTags('waitlist')
@Controller('waitlist')
export class WaitlistController {
  constructor(private readonly waitlistService: WaitlistService) {}

  @Post('join')
  @ApiOperation({ summary: 'Join the waitlist' })
  @ApiConflictResponse({ description: 'Email already on waitlist' })
  @ApiBadRequestResponse({ description: 'Validation failed' })
  join(@Body() dto: JoinWaitlistDto) {
    return this.waitlistService.join(dto);
  }

  @Get('check/:username')
  @ApiOperation({ summary: 'Check username availability' })
  @ApiOkResponse({ schema: { example: { available: true } } })
  checkUsername(@Param('username') username: string) {
    return this.waitlistService.checkUsername(username);
  }

  @Get('stats')
  @ApiOperation({ summary: 'Waitlist total count' })
  getStats() {
    return this.waitlistService.getStats();
  }

  @Get('position')
  @ApiOperation({ summary: 'Get position by email (requires unsubscribe token)' })
  @ApiQuery({ name: 'email', required: true })
  @ApiQuery({ name: 'token', required: true, description: 'Unsubscribe token issued at registration' })
  @ApiBadRequestResponse({ description: 'Missing email or token' })
  @ApiUnauthorizedResponse({ description: 'Invalid token' })
  getPosition(@Query('email') email: string, @Query('token') token: string) {
    if (!email || !token) {
      throw new BadRequestException('email and token are required');
    }
    return this.waitlistService.getPosition(email, token);
  }

  /**
   * One-click unsubscribe — no login required.
   * Token is embedded in all waitlist emails as ?token=xxx
   */
  @Post('unsubscribe')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({ summary: 'Unsubscribe from waitlist emails (no login required)' })
  @ApiQuery({ name: 'token', required: true, description: 'Unsubscribe token from email link' })
  @ApiUnauthorizedResponse({ description: 'Invalid token' })
  unsubscribe(@Query('token') token: string) {
    return this.waitlistService.unsubscribe(token);
  }

  /**
   * Full GDPR deletion — removes all PII.
   * Requires email + unsubscribe token (no login required).
   */
  @Delete()
  @HttpCode(HttpStatus.OK)
  @ApiOperation({ summary: 'Delete waitlist entry and all PII (GDPR)' })
  @ApiQuery({ name: 'email', required: true })
  @ApiQuery({ name: 'token', required: true })
  @ApiUnauthorizedResponse({ description: 'Invalid token or email' })
  deleteEntry(@Query('email') email: string, @Query('token') token: string) {
    return this.waitlistService.deleteEntry(email, token);
  }
}
