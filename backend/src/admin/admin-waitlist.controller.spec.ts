import { Test, TestingModule } from '@nestjs/testing';
import { ExecutionContext, ForbiddenException } from '@nestjs/common';
import { Reflector } from '@nestjs/core';
import { AdminWaitlistController } from './admin-waitlist.controller';
import { WaitlistService } from '../waitlist/waitlist.service';
import { JwtAuthGuard } from '../auth/guards/jwt.guard';
import { RolesGuard } from '../auth/guards/roles.guard';
import { MerchantRole } from '../merchants/entities/merchant.entity';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function mockExecutionContext(role: MerchantRole | null): ExecutionContext {
  return {
    getHandler: () => AdminWaitlistController.prototype.exportCsv,
    getClass: () => AdminWaitlistController,
    switchToHttp: () => ({
      getRequest: () => ({
        user: role ? { merchantId: 'test-id', email: 'test@example.com', role } : undefined,
      }),
    }),
  } as unknown as ExecutionContext;
}

// ---------------------------------------------------------------------------
// RolesGuard unit tests (Issue #4)
// ---------------------------------------------------------------------------

describe('RolesGuard', () => {
  let guard: RolesGuard;
  let reflector: Reflector;

  beforeEach(async () => {
    const module: TestingModule = await Test.createTestingModule({
      providers: [RolesGuard, Reflector],
    }).compile();

    guard = module.get<RolesGuard>(RolesGuard);
    reflector = module.get<Reflector>(Reflector);
  });

  it('allows a user with MerchantRole.ADMIN', () => {
    jest
      .spyOn(reflector, 'getAllAndOverride')
      .mockReturnValue([MerchantRole.ADMIN]);

    const ctx = mockExecutionContext(MerchantRole.ADMIN);
    expect(guard.canActivate(ctx)).toBe(true);
  });

  it('allows a user with MerchantRole.SUPERADMIN even when ADMIN is required', () => {
    jest
      .spyOn(reflector, 'getAllAndOverride')
      .mockReturnValue([MerchantRole.ADMIN]);

    const ctx = mockExecutionContext(MerchantRole.SUPERADMIN);
    expect(guard.canActivate(ctx)).toBe(true);
  });

  it('throws ForbiddenException for MerchantRole.MERCHANT', () => {
    jest
      .spyOn(reflector, 'getAllAndOverride')
      .mockReturnValue([MerchantRole.ADMIN]);

    const ctx = mockExecutionContext(MerchantRole.MERCHANT);
    expect(() => guard.canActivate(ctx)).toThrow(ForbiddenException);
  });

  it('throws ForbiddenException when no user is present (unauthenticated)', () => {
    jest
      .spyOn(reflector, 'getAllAndOverride')
      .mockReturnValue([MerchantRole.ADMIN]);

    const ctx = mockExecutionContext(null);
    expect(() => guard.canActivate(ctx)).toThrow(ForbiddenException);
  });

  it('allows access when no @Roles decorator is present', () => {
    jest.spyOn(reflector, 'getAllAndOverride').mockReturnValue(undefined);

    const ctx = mockExecutionContext(MerchantRole.MERCHANT);
    expect(guard.canActivate(ctx)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// AdminWaitlistController + guard integration
// ---------------------------------------------------------------------------

describe('AdminWaitlistController', () => {
  let controller: AdminWaitlistController;
  let waitlistService: Partial<WaitlistService>;

  beforeEach(async () => {
    waitlistService = {
      exportCsv: jest.fn().mockResolvedValue('email,username\n"a@b.com","alice"'),
    };

    const module: TestingModule = await Test.createTestingModule({
      controllers: [AdminWaitlistController],
      providers: [
        { provide: WaitlistService, useValue: waitlistService },
        Reflector,
        RolesGuard,
      ],
    })
      // Skip actual JWT validation in unit tests.
      .overrideGuard(JwtAuthGuard)
      .useValue({ canActivate: () => true })
      .compile();

    controller = module.get<AdminWaitlistController>(AdminWaitlistController);
  });

  it('is defined', () => {
    expect(controller).toBeDefined();
  });

  it('calls waitlistService.exportCsv with correct options and returns the CSV', async () => {
    const res = {
      setHeader: jest.fn(),
    } as any;

    const csv = await controller.exportCsv('GB', '2024-01-01', '2024-12-31', 'false', res);

    expect(waitlistService.exportCsv).toHaveBeenCalledWith({
      country: 'GB',
      dateFrom: '2024-01-01',
      dateTo: '2024-12-31',
      includeUnsubscribed: false,
    });
    expect(res.setHeader).toHaveBeenCalledWith('Content-Type', 'text/csv');
    expect(res.setHeader).toHaveBeenCalledWith(
      'Content-Disposition',
      'attachment; filename="waitlist-export.csv"',
    );
    expect(csv).toBe('email,username\n"a@b.com","alice"');
  });

  it('defaults includeUnsubscribed to false when not provided', async () => {
    const res = {
      setHeader: jest.fn(),
    } as any;

    await controller.exportCsv(undefined, undefined, undefined, undefined, res);

    expect(waitlistService.exportCsv).toHaveBeenCalledWith({
      country: undefined,
      dateFrom: undefined,
      dateTo: undefined,
      includeUnsubscribed: false,
    });
  });

  it('propagates errors from waitlistService.exportCsv to the exception filter', async () => {
    const res = {
      setHeader: jest.fn(),
    } as any;
    const error = new Error('export failed');
    (waitlistService.exportCsv as jest.Mock).mockRejectedValueOnce(error);

    await expect(
      controller.exportCsv('GB', undefined, undefined, undefined, res),
    ).rejects.toThrow('export failed');
  });
});
