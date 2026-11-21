import { UnauthorizedException } from '@nestjs/common';
import { ApiKeyStrategy } from './api-key.strategy';
import { AuthService } from '../auth.service';
import { MerchantRole } from '../../merchants/entities/merchant.entity';

describe('ApiKeyStrategy', () => {
  let strategy: ApiKeyStrategy;
  let authService: { findMerchantByApiKey: jest.Mock };

  beforeEach(() => {
    authService = { findMerchantByApiKey: jest.fn() };
    strategy = new ApiKeyStrategy(authService as unknown as AuthService);
  });

  const buildRequest = (apiKey?: string) =>
    ({ headers: apiKey ? { 'x-api-key': apiKey } : {} }) as any;

  it('throws when API key header is missing', async () => {
    await expect(strategy.validate(buildRequest())).rejects.toBeInstanceOf(
      UnauthorizedException,
    );
  });

  it('throws when merchant is not found', async () => {
    authService.findMerchantByApiKey.mockResolvedValue(null);
    await expect(strategy.validate(buildRequest('bad-key'))).rejects.toBeInstanceOf(
      UnauthorizedException,
    );
  });

  it('returns a principal with a defined role', async () => {
    authService.findMerchantByApiKey.mockResolvedValue({
      id: 'merchant-1',
      email: 'merchant@example.com',
      role: MerchantRole.MERCHANT,
    });

    const principal = await strategy.validate(buildRequest('valid-key'));

    expect(principal).toEqual({
      merchantId: 'merchant-1',
      email: 'merchant@example.com',
      role: MerchantRole.MERCHANT,
    });
    expect(principal.role).toBeDefined();
  });

  it('defaults role to merchant when merchant.role is undefined', async () => {
    authService.findMerchantByApiKey.mockResolvedValue({
      id: 'merchant-2',
      email: 'legacy@example.com',
    });

    const principal = await strategy.validate(buildRequest('valid-key'));

    expect(principal.role).toBe(MerchantRole.MERCHANT);
    expect(principal.role).toBeDefined();
  });
});
