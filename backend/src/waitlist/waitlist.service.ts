import { Injectable, NotFoundException } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { Repository, Between, FindOptionsWhere } from 'typeorm';
import { WaitlistEntry } from './entities/waitlist.entity';
import { JoinWaitlistDto } from './dto/join-waitlist.dto';

export interface ExportCsvOptions {
  dateFrom?: string;
  dateTo?: string;
  country?: string;
  includeUnsubscribed?: boolean;
}

@Injectable()
export class WaitlistService {
  constructor(
    @InjectRepository(WaitlistEntry)
    private readonly waitlistRepo: Repository<WaitlistEntry>,
  ) {}

  async join(dto: JoinWaitlistDto): Promise<WaitlistEntry> {
    const existing = await this.waitlistRepo.findOne({
      where: [{ email: dto.email }, { username: dto.username }],
    });
    if (existing) {
      return existing;
    }

    const entry = this.waitlistRepo.create({
      email: dto.email,
      username: dto.username,
      businessName: dto.businessName,
      country: dto.country,
      referralCode: this.generateReferralCode(),
      unsubscribeToken: this.generateUnsubscribeToken(),
    });

    return this.waitlistRepo.save(entry);
  }

  async findByEmail(email: string): Promise<WaitlistEntry | null> {
    return this.waitlistRepo.findOne({ where: { email } });
  }

  async unsubscribe(token: string): Promise<void> {
    const entry = await this.waitlistRepo.findOne({
      where: { unsubscribeToken: token },
    });
    if (!entry) {
      throw new NotFoundException('Invalid unsubscribe token');
    }
    entry.unsubscribed = true;
    await this.waitlistRepo.save(entry);
  }

  async getPosition(id: string): Promise<number> {
    const entry = await this.waitlistRepo.findOne({ where: { id } });
    if (!entry) {
      throw new NotFoundException('Waitlist entry not found');
    }
    return this.waitlistRepo.count({
      where: { createdAt: Between(new Date(0), entry.createdAt) },
    });
  }

  async exportCsv(options: ExportCsvOptions = {}): Promise<string> {
    const where: FindOptionsWhere<WaitlistEntry> = {};

    if (options.dateFrom && options.dateTo) {
      where.createdAt = Between(new Date(options.dateFrom), new Date(options.dateTo));
    } else if (options.dateFrom) {
      where.createdAt = Between(new Date(options.dateFrom), new Date());
    } else if (options.dateTo) {
      where.createdAt = Between(new Date(0), new Date(options.dateTo));
    }

    if (options.country) {
      where.country = options.country;
    }

    if (!options.includeUnsubscribed) {
      where.unsubscribed = false;
    }

    const entries = await this.waitlistRepo.find({
      where,
      order: { createdAt: 'ASC' },
    });

    const header = 'email,username,businessName,country,referrals,createdAt';
    const rows = entries.map((e) =>
      [
        e.email,
        e.username,
        e.businessName ?? '',
        e.country ?? '',
        e.referrals,
        e.createdAt.toISOString(),
      ].join(','),
    );

    return [header, ...rows].join('\n');
  }

  private generateReferralCode(): string {
    return Math.random().toString(36).substring(2, 10).toUpperCase();
  }

  private generateUnsubscribeToken(): string {
    return Math.random().toString(36).substring(2) + Date.now().toString(36);
  }
}
