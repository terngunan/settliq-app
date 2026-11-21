import { Test, TestingModule } from '@nestjs/testing';
import { getRepositoryToken } from '@nestjs/typeorm';
import { JwtService } from '@nestjs/jwt';
import { BadRequestException, UnauthorizedException } from '@nestjs/common';
import * as bcrypt from 'bcrypt';
import { AuthService } from './auth.service';
import { Merchant } from '../merchants/entities/merchant.entity';
import { MerchantStatus } from '../merchants/entities/merchant.entity';
import { RefreshToken } from './entities/refresh-token.entity';

jest.mock('bcrypt');

describe('AuthService', () => {
  let service: AuthService;
  let repository: any;
  let refreshTokenRepo: any;
  let jwtService: any;

  const mockMerchant = (overrides: Partial<Merchant> = {}): Merchant =>
    ({
      id: 'merchant-1',
      email: 'merchant@example.com',
      password: 'hashed-password',
      passwordResetToken: null,
      passwordResetExpiry: null,
      ...overrides,
    } as Merchant);

  beforeEach(async () => {
    repository = {
      findOne: jest.fn(),
      save: jest.fn(),
      create: jest.fn(),
      update: jest.fn(),
    };
    refreshTokenRepo = {
      findOne: jest.fn(),
      save: jest.fn(),
      create: jest.fn(),
      delete: jest.fn(),
    };
    jwtService = {
      sign: jest.fn().mockReturnValue('signed-token'),
    };

    const module: TestingModule = await Test.createTestingModule({
      providers: [
        AuthService,
        { provide: getRepositoryToken(Merchant), useValue: repository },
        {
          provide: getRepositoryToken(RefreshToken),
          useValue: refreshTokenRepo,
        },
        { provide: JwtService, useValue: jwtService },
      ],
    }).compile();

    service = module.get<AuthService>(AuthService);
  });

  afterEach(() => {
    jest.clearAllMocks();
  });

  describe('register', () => {
    it('creates the merchant with PENDING status', async () => {
      repository.findOne.mockResolvedValue(null);
      repository.create.mockImplementation((data: Partial<Merchant>) =>
        mockMerchant(data),
      );
      repository.save.mockImplementation(async (m: Merchant) => m);
      (bcrypt.hash as jest.Mock).mockResolvedValue('hashed-password');

      const merchant = await service.register({
        email: 'new@example.com',
        password: 'password123',
        businessName: 'New Biz',
        businessType: 'retail',
        country: 'US',
      } as any);

      expect(repository.create).toHaveBeenCalledWith(
        expect.objectContaining({ status: MerchantStatus.PENDING }),
      );
      expect(merchant.status).toBe(MerchantStatus.PENDING);
    });
  });

  describe('forgotPassword', () => {
    it('generates a reset token and persists it with an expiry', async () => {
      const merchant = mockMerchant();
      repository.findOne.mockResolvedValue(merchant);
      repository.save.mockImplementation(async (m: Merchant) => m);

      const result = await service.forgotPassword(merchant.email);

      expect(repository.findOne).toHaveBeenCalledWith({
        where: { email: merchant.email },
      });
      expect(repository.save).toHaveBeenCalled();
      expect(merchant.passwordResetToken).toBeDefined();
      expect(merchant.passwordResetToken).not.toBeNull();
      expect(merchant.passwordResetExpiry).toBeInstanceOf(Date);
      expect(merchant.passwordResetExpiry!.getTime()).toBeGreaterThan(
        Date.now(),
      );
      expect(result).toBeDefined();
    });

    it('does not reveal whether the email exists', async () => {
      repository.findOne.mockResolvedValue(null);

      await expect(
        service.forgotPassword('unknown@example.com'),
      ).resolves.toBeDefined();
      expect(repository.save).not.toHaveBeenCalled();
    });
  });

  describe('resetPassword', () => {
    it('atomically updates the password and clears the reset token', async () => {
      const merchant = mockMerchant({
        passwordResetToken: 'valid-token',
        passwordResetExpiry: new Date(Date.now() + 60 * 60 * 1000),
      });
      repository.findOne.mockResolvedValue(merchant);
      repository.update.mockResolvedValue({ affected: 1 });
      (bcrypt.hash as jest.Mock).mockResolvedValue('new-hashed-password');

      await service.resetPassword('valid-token', 'newPassword123');

      expect(bcrypt.hash).toHaveBeenCalledWith('newPassword123', 12);
      expect(repository.update).toHaveBeenCalledWith(
        { id: merchant.id, passwordResetToken: 'valid-token' },
        expect.objectContaining({
          password: 'new-hashed-password',
          passwordResetToken: null,
          passwordResetExpiry: null,
        }),
      );
      expect(repository.save).not.toHaveBeenCalled();
    });

    it('rejects an expired reset token with BadRequestException', async () => {
      const merchant = mockMerchant({
        passwordResetToken: 'expired-token',
        passwordResetExpiry: new Date(Date.now() - 60 * 1000),
      });
      repository.findOne.mockResolvedValue(merchant);

      await expect(
        service.resetPassword('expired-token', 'newPassword123'),
      ).rejects.toBeInstanceOf(BadRequestException);
      expect(repository.update).not.toHaveBeenCalled();
      expect(repository.save).not.toHaveBeenCalled();
    });

    it('rejects an invalid reset token with BadRequestException', async () => {
      repository.findOne.mockResolvedValue(null);

      await expect(
        service.resetPassword('invalid-token', 'newPassword123'),
      ).rejects.toBeInstanceOf(BadRequestException);
      expect(repository.update).not.toHaveBeenCalled();
      expect(repository.save).not.toHaveBeenCalled();
    });

    it('rejects when the conditional update affects no rows (token already consumed)', async () => {
      const merchant = mockMerchant({
        passwordResetToken: 'valid-token',
        passwordResetExpiry: new Date(Date.now() + 60 * 60 * 1000),
      });
      repository.findOne.mockResolvedValue(merchant);
      repository.update.mockResolvedValue({ affected: 0 });
      (bcrypt.hash as jest.Mock).mockResolvedValue('new-hashed-password');

      await expect(
        service.resetPassword('valid-token', 'newPassword123'),
      ).rejects.toBeInstanceOf(BadRequestException);
    });

    it('allows only one of two concurrent resets with the same token to succeed', async () => {
      const merchant = mockMerchant({
        passwordResetToken: 'valid-token',
        passwordResetExpiry: new Date(Date.now() + 60 * 60 * 1000),
      });
      repository.findOne.mockResolvedValue(merchant);
      (bcrypt.hash as jest.Mock).mockResolvedValue('new-hashed-password');

      let consumed = false;
      repository.update.mockImplementation(async () => {
        if (consumed) {
          return { affected: 0 };
        }
        consumed = true;
        return { affected: 1 };
      });

      const results = await Promise.allSettled([
        service.resetPassword('valid-token', 'newPassword123'),
        service.resetPassword('valid-token', 'newPassword123'),
      ]);

      const fulfilled = results.filter((r) => r.status === 'fulfilled');
      const rejected = results.filter((r) => r.status === 'rejected');

      expect(fulfilled).toHaveLength(1);
      expect(rejected).toHaveLength(1);
      expect((rejected[0] as PromiseRejectedResult).reason).toBeInstanceOf(
        BadRequestException,
      );
    });
  });

  describe('login', () => {
    it('throws UnauthorizedException for invalid credentials', async () => {
      repository.findOne.mockResolvedValue(null);

      await expect(
        service.login({ email: 'nope@example.com', password: 'wrong' } as any),
      ).rejects.toBeInstanceOf(UnauthorizedException);
    });
  });
});
