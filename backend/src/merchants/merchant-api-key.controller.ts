import {
  Controller,
  Delete,
  HttpCode,
  HttpStatus,
  Post,
  Req,
  UnauthorizedException,
  UseGuards,
} from '@nestjs/common';
import { ApiBearerAuth, ApiOperation, ApiResponse, ApiTags } from '@nestjs/swagger';
import { Request } from 'express';
import { AuthService } from '../auth/auth.service';
import { JwtAuthGuard } from '../auth/guards/jwt.guard';

interface AuthenticatedRequest extends Request {
  user?: {
    id?: string;
    merchantId?: string;
    sub?: string;
  };
}

@ApiTags('merchants')
@ApiBearerAuth()
@Controller('merchants/api-key')
@UseGuards(JwtAuthGuard)
export class MerchantApiKeyController {
  constructor(private readonly authService: AuthService) {}

  @Post()
  @HttpCode(HttpStatus.CREATED)
  @ApiOperation({ summary: 'Generate a new API key for the authenticated merchant' })
  @ApiResponse({ status: 201, description: 'API key generated. The full key is returned only once.' })
  @ApiResponse({ status: 401, description: 'Unauthorized' })
  async generate(@Req() req: AuthenticatedRequest) {
    const merchantId = this.resolveMerchantId(req);
    return this.authService.generateApiKey(merchantId);
  }

  @Post('rotate')
  @HttpCode(HttpStatus.CREATED)
  @ApiOperation({ summary: 'Rotate the merchant API key with a grace period' })
  @ApiResponse({ status: 201, description: 'API key rotated. The new full key is returned only once.' })
  @ApiResponse({ status: 401, description: 'Unauthorized' })
  async rotate(@Req() req: AuthenticatedRequest) {
    const merchantId = this.resolveMerchantId(req);
    return this.authService.rotateApiKey(merchantId);
  }

  @Delete()
  @HttpCode(HttpStatus.NO_CONTENT)
  @ApiOperation({ summary: 'Revoke the active API key for the authenticated merchant' })
  @ApiResponse({ status: 204, description: 'API key revoked' })
  @ApiResponse({ status: 401, description: 'Unauthorized' })
  async revoke(@Req() req: AuthenticatedRequest) {
    const merchantId = this.resolveMerchantId(req);
    await this.authService.revokeApiKey(merchantId);
  }

  private resolveMerchantId(req: AuthenticatedRequest): string {
    const merchantId = req.user?.merchantId ?? req.user?.id ?? req.user?.sub;
    if (!merchantId) {
      throw new UnauthorizedException('Merchant identity not found on request');
    }
    return merchantId;
  }
}
