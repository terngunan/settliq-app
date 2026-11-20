import { Injectable, UnauthorizedException } from '@nestjs/common';
import { PassportStrategy } from '@nestjs/passport';
import { Strategy } from 'passport-custom';
import { Request } from 'express';
import { AuthService } from '../auth.service';
import { MerchantRole } from '../../merchants/entities/merchant.entity';

@Injectable()
export class ApiKeyStrategy extends PassportStrategy(Strategy, 'api-key') {
  constructor(private readonly authService: AuthService) {
    super();
  }

  async validate(req: Request): Promise<{
    merchantId: string;
    email: string;
    role: MerchantRole;
  }> {
    const rawKey = req.headers['x-api-key'] as string;
    if (!rawKey) throw new UnauthorizedException('Missing API key');

    const merchant = await this.authService.findMerchantByApiKey(rawKey.trim());
    if (!merchant) throw new UnauthorizedException('Invalid API key');

    return {
      merchantId: merchant.id,
      email: merchant.email,
      role: merchant.role ?? MerchantRole.MERCHANT,
    };
  }
}
