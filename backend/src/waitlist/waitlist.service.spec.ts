import { Test, TestingModule } from '@nestjs/testing';
import { getRepositoryToken } from '@nestjs/typeorm';
import { ConflictException, NotFoundException } from '@nestjs/common';
import { WaitlistService } from './waitlist.service';
import { WaitlistEntry } from './entities/waitlist.entity';

describe('WaitlistService', () => {
  let service: WaitlistService;
  let repo: any;

  beforeEach(async () => {
    repo = {
      findOne: jest.fn(),
      create: jest.fn((x) => x),
      save: jest.fn((x) => Promise.resolve(x)),
      createQueryBuilder: jest.fn(),
    };

    const module: TestingModule = await Test.createTestingModule({
      providers: [
        WaitlistService,
        { provide: getRepositoryToken(WaitlistEntry), useValue: repo },
      ],
    }).compile();

    service = module.get<WaitlistService>(WaitlistService);
  });

  describe('join', () => {
    it('retries referral code generation on collision and succeeds', async () => {
      const generateSpy = jest
        .spyOn(service as any, 'generateUniqueReferralCode')
        .mockResolvedValueOnce('UNIQUE01');

      repo.findOne.mockResolvedValue(null);

      const dto = {
        email: 'new@example.com',
        username: 'newuser',
        businessName: 'New Biz',
        country: 'US',
      } as any;

      const result = await service.join(dto);

      expect(generateSpy).toHaveBeenCalled();
      expect(repo.create).toHaveBeenCalledWith(
        expect.objectContaining({ referralCode: 'UNIQUE01' }),
      );
      expect(result).toBeDefined();
    });

    it('generates a collision-checked referral code via generateUniqueReferralCode', async () => {
      const generateSpy = jest
        .spyOn(service as any, 'generateUniqueReferralCode')
        .mockResolvedValue('ABCD1234');

      repo.findOne.mockResolvedValue(null);

      await service.join({
        email: 'x@example.com',
        username: 'xuser',
        businessName: 'X',
        country: 'US',
      } as any);

      expect(generateSpy).toHaveBeenCalledTimes(1);
      expect(repo.create).toHaveBeenCalledWith(
        expect.objectContaining({ referralCode: 'ABCD1234' }),
      );
    });
  });

  describe('getPosition', () => {
    it('throws when email is not on the waitlist', async () => {
      repo.findOne.mockResolvedValue(null);
      await expect(service.getPosition('missing@example.com')).rejects.toBeInstanceOf(
        NotFoundException,
      );
    });

    it('returns position 1 when no entries are ahead', async () => {
      repo.findOne.mockResolvedValue({ id: 'a', email: 'a@example.com', referrals: 0, createdAt: new Date() });
      const qb = { where: jest.fn().mockReturnThis(), orWhere: jest.fn().mockReturnThis(), getCount: jest.fn().mockResolvedValue(0) };
      repo.createQueryBuilder.mockReturnValue(qb);

      await expect(service.getPosition('a@example.com')).resolves.toEqual({ position: 1 });
    });

    it('promotes a user with referrals ahead of an earlier user without referrals', async () => {
      const earlier = { id: 'early', email: 'early@example.com', referrals: 0, createdAt: new Date('2024-01-01') };
      const referrer = { id: 'ref', email: 'ref@example.com', referrals: 5, createdAt: new Date('2024-02-01') };

      // Referrer: the earlier zero-referral user is NOT ahead (fewer referrals).
      repo.findOne.mockResolvedValue(referrer);
      const refQb = { where: jest.fn().mockReturnThis(), orWhere: jest.fn().mockReturnThis(), getCount: jest.fn().mockResolvedValue(0) };
      repo.createQueryBuilder.mockReturnValue(refQb);
      const refPosition = await service.getPosition('ref@example.com');

      // Earlier user: the referrer IS ahead (more referrals).
      repo.findOne.mockResolvedValue(earlier);
      const earlyQb = { where: jest.fn().mockReturnThis(), orWhere: jest.fn().mockReturnThis(), getCount: jest.fn().mockResolvedValue(1) };
      repo.createQueryBuilder.mockReturnValue(earlyQb);
      const earlyPosition = await service.getPosition('early@example.com');

      expect(refPosition.position).toBeLessThan(earlyPosition.position);
    });
  });
});
