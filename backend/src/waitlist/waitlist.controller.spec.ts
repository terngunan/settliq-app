import { Test, TestingModule } from '@nestjs/testing';
import { ThrottlerModule } from '@nestjs/throttler';
import { WaitlistController } from './waitlist.controller';
import { WaitlistService } from './waitlist.service';

// ── Helpers ────────────────────────────────────────────────────────────────

function makeServiceMock(): jest.Mocked<WaitlistService> {
  return {
    join: jest.fn(),
    checkUsername: jest.fn(),
    getStats: jest.fn(),
    getPosition: jest.fn(),
    unsubscribe: jest.fn(),
    deleteEntry: jest.fn(),
    exportCsv: jest.fn(),
  } as unknown as jest.Mocked<WaitlistService>;
}

// ── Tests ──────────────────────────────────────────────────────────────────

describe('WaitlistController', () => {
  let controller: WaitlistController;
  let service: jest.Mocked<WaitlistService>;

  beforeEach(async () => {
    service = makeServiceMock();

    const module: TestingModule = await Test.createTestingModule({
      imports: [ThrottlerModule.forRoot([{ ttl: 60000, limit: 60 }])],
      controllers: [WaitlistController],
      providers: [{ provide: WaitlistService, useValue: service }],
    }).compile();

    controller = module.get<WaitlistController>(WaitlistController);
  });

  // ── join ──────────────────────────────────────────────────────────────

  describe('join', () => {
    it('delegates to service.join and returns result', async () => {
      const dto = { email: 'founder@example.com' };
      // service.join already returns a sanitised DTO (no unsubscribeToken)
      const entry = { id: '1', email: dto.email };
      service.join.mockResolvedValue(entry as any);

      const result = await controller.join(dto as any);

      expect(service.join).toHaveBeenCalledWith(dto);
      expect(result).toBe(entry);
    });

    it('does not expose unsubscribeToken in the response', async () => {
      const dto = { email: 'founder@example.com' };
      const sanitised = { id: '1', email: dto.email, referralCode: 'ABCD1234' };
      service.join.mockResolvedValue(sanitised as any);

      const result = await controller.join(dto as any);

      expect(result).not.toHaveProperty('unsubscribeToken');
    });
  });

  // ── checkUsername ─────────────────────────────────────────────────────

  describe('checkUsername', () => {
    it('delegates to service.checkUsername and returns result', async () => {
      service.checkUsername.mockResolvedValue({ available: true });

      const result = await controller.checkUsername('free');

      expect(service.checkUsername).toHaveBeenCalledWith('free');
      expect(result).toEqual({ available: true });
    });
  });

  // ── getStats ──────────────────────────────────────────────────────────

  describe('getStats', () => {
    it('delegates to service.getStats and returns result', async () => {
      service.getStats.mockResolvedValue({ total: 100 });

      const result = await controller.getStats();

      expect(result).toEqual({ total: 100 });
    });
  });

  // ── getPosition ───────────────────────────────────────────────────────

  describe('getPosition', () => {
    it('delegates to service.getPosition with the provided email', async () => {
      service.getPosition.mockResolvedValue({ position: 5 });

      const result = await controller.getPosition('founder@example.com');

      expect(service.getPosition).toHaveBeenCalledWith('founder@example.com');
      expect(result).toEqual({ position: 5 });
    });
  });

  // ── unsubscribe ───────────────────────────────────────────────────────

  describe('unsubscribe', () => {
    it('delegates to service.unsubscribe with the token', async () => {
      service.unsubscribe.mockResolvedValue({ message: 'Successfully unsubscribed' });

      const result = await controller.unsubscribe('tok-abc');

      expect(service.unsubscribe).toHaveBeenCalledWith('tok-abc');
      expect(result).toEqual({ message: 'Successfully unsubscribed' });
    });
  });

  // ── deleteEntry ───────────────────────────────────────────────────────

  describe('deleteEntry', () => {
    it('delegates to service.deleteEntry with email and token', async () => {
      service.deleteEntry.mockResolvedValue({ message: 'Deleted' });

      const result = await controller.deleteEntry('founder@example.com', 'tok-abc');

      expect(service.deleteEntry).toHaveBeenCalledWith('founder@example.com', 'tok-abc');
      expect(result).toEqual({ message: 'Deleted' });
    });
  });
});
