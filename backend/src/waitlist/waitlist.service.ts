import {
  ConflictException,
  Injectable,
  NotFoundException,
  UnauthorizedException,
} from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { Between, FindOptionsWhere, MoreThanOrEqual, LessThanOrEqual, Repository } from 'typeorm';
import { randomBytes } from 'crypto';
import { WaitlistEntry } from './entities/waitlist.entity';
import { JoinWaitlistDto } from './dto/join-waitlist.dto';
import { WaitlistEntryResponseDto } from './dto/waitlist-entry-response.dto';

export interface ExportCsvOptions {
  dateFrom?: string;
  dateTo?: string;
}

@Injectable()
export class WaitlistService {
  constructor(
    @InjectRepository(WaitlistEntry)
    private readonly waitlistRepo: Repository<WaitlistEntry>,
  ) {}

  async join(dto: JoinWaitlistDto): Promise<WaitlistEntryResponseDto> {
    const existing = await this.waitlistRepo.findOne({ where: { email: dto.email } });
    if (existing) {
      throw new ConflictException('Email is already on the waitlist');
    }

    const usernameTaken = await this.waitlistRepo.findOne({
      where: { username: dto.username },
    });
    if (usernameTaken) {
      throw new ConflictException('Username is already taken');
    }

    const entry = this.waitlistRepo.create({
      email: dto.email,
      username: dto.username,
      businessName: dto.businessName,
      country: dto.country,
      referralCode: randomBytes(4).toString('hex').toUpperCase(),
      unsubscribeToken: randomBytes(16).toString('hex'),
    });

    let saved: WaitlistEntry;
    try {
      saved = await this.waitlistRepo.save(entry);
    } catch (error) {
      // Fallback for race conditions where a concurrent request inserts the
      // same username between the check above and save(). Map the Postgres
      // unique violation (23505) to a 409 instead of leaking a 500.
      if (this.isUniqueViolation(error)) {
        throw new ConflictException('Username is already taken');
      }
      throw error;
    }

    if (dto.referralCode) {
      const referrer = await this.waitlistRepo.findOne({
        where: { referralCode: dto.referralCode },
      });
      if (referrer && referrer.email !== dto.email) {
        await this.waitlistRepo.increment({ id: referrer.id }, 'referrals', 1);
      }
    }

    // Return a sanitised response – unsubscribeToken is a secret used only
    // for one-click unsubscribe links in emails and must never be exposed in
    // HTTP response bodies.
    const { unsubscribeToken: _stripped, ...response } = saved as WaitlistEntry & {
      businessName?: string;
      country?: string;
    };
    return response as WaitlistEntryResponseDto;
  }

  /**
   * Detect a Postgres unique constraint violation (SQLSTATE 23505) surfaced
   * by TypeORM as a QueryFailedError.
   */
  private isUniqueViolation(error: unknown): boolean {
    if (!error || typeof error !== 'object') return false;
    const code = (error as { code?: string }).code;
    if (code === '23505') return true;
    const driverError = (error as { driverError?: { code?: string } }).driverError;
    return driverError?.code === '23505';
  }

  /**
   * Generate a referral code that does not already exist in the database.
   * Retries up to REFERRAL_CODE_MAX_ATTEMPTS times on collision before failing.
   */
  private async generateUniqueReferralCode(): Promise<string> {
    for (let attempt = 0; attempt < REFERRAL_CODE_MAX_ATTEMPTS; attempt++) {
      const candidate = randomBytes(6).toString('hex').toUpperCase();
      const collision = await this.waitlistRepo.findOne({
        where: { referralCode: candidate },
      });
      if (!collision) return candidate;
    }
    throw new ConflictException(
      'Unable to generate a unique referral code, please try again',
    );
  }

  async checkUsername(username: string): Promise<{ available: boolean }> {
    const existing = await this.waitlistRepo.findOne({ where: { username } });
    return { available: !existing };
  }

  async getStats(): Promise<{ total: number }> {
    const total = await this.waitlistRepo.count();
    return { total };
  }

  async getPosition(email: string): Promise<{ position: number }> {
    const entry = await this.waitlistRepo.findOne({ where: { email } });
    if (!entry) {
      throw new NotFoundException('Email is not on the waitlist');
    }

    // Rank by referrals DESC, then createdAt ASC (stable tie-break on id) so
    // accumulated referrals advance a user's queue position.
    const ahead = await this.waitlistRepo
      .createQueryBuilder('w')
      .where('w.referrals > :referrals', { referrals: entry.referrals })
      .orWhere(
        'w.referrals = :referrals AND (w.createdAt < :createdAt OR (w.createdAt = :createdAt AND w.id <= :id))',
        { referrals: entry.referrals, createdAt: entry.createdAt, id: entry.id },
      )
      .getCount();

    return { position: ahead };
  }

  async unsubscribe(token: string): Promise<{ message: string }> {
    const entry = await this.waitlistRepo.findOne({ where: { unsubscribeToken: token } });
    if (!entry) {
      throw new UnauthorizedException('Invalid unsubscribe token');
    }
    await this.waitlistRepo.update(entry.id, { unsubscribed: true });
    return { message: 'You have been unsubscribed' };
  }

  async deleteEntry(email: string, token: string): Promise<{ message: string }> {
    const entry = await this.waitlistRepo.findOne({
      where: { email, unsubscribeToken: token },
    });
    if (!entry) {
      throw new UnauthorizedException('Invalid email or token');
    }
    await this.waitlistRepo.delete(entry.id);
    return { message: 'Your data has been deleted' };
  }

  async exportCsv(opts: ExportCsvOptions = {}): Promise<string> {
    const where: FindOptionsWhere<WaitlistEntry> = {};

    if (opts.dateFrom && opts.dateTo) {
      where.createdAt = Between(new Date(opts.dateFrom), new Date(opts.dateTo));
    } else if (opts.dateFrom) {
      where.createdAt = MoreThanOrEqual(new Date(opts.dateFrom));
    } else if (opts.dateTo) {
      where.createdAt = LessThanOrEqual(new Date(opts.dateTo));
    }

    const entries = await this.waitlistRepo.find({
      where,
      order: { createdAt: 'ASC' },
    });

    const header = 'email,username,businessName,country,referrals,createdAt';
    const rows = entries.map((e) =>
      [e.email, e.username, e.businessName, e.country, e.referrals, e.createdAt.toISOString()]
        .map((v) => `"${String(v ?? '').replace(/"/g, '""')}"`)
        .join(','),
    );

    return [header, ...rows].join('\n');
  }
}
