import {
  Column,
  CreateDateColumn,
  Entity,
  PrimaryGeneratedColumn,
  UpdateDateColumn,
} from 'typeorm';

export enum MerchantStatus {
  PENDING = 'pending',
  ACTIVE = 'active',
  SUSPENDED = 'suspended',
}

@Entity('merchants')
export class Merchant {
  @PrimaryGeneratedColumn('uuid')
  id: string;

  @Column({ unique: true })
  email: string;

  @Column({ nullable: true })
  name?: string;

  @Column({ nullable: true })
  businessName?: string;

  @Column({ nullable: true })
  businessType?: string;

  @Column({ nullable: true })
  country?: string;

  @Column({ nullable: true })
  passwordHash?: string;

  @Column({ nullable: true, select: false })
  passwordResetToken?: string;

  @Column({ type: 'timestamptz', nullable: true })
  passwordResetExpiry?: Date;

  @Column({ nullable: true, select: false })
  apiKeyHash?: string;

  @Column({ nullable: true, select: false })
  apiKeyLookupHash?: string;

  @Column({ nullable: true })
  apiKeyLastFour?: string;

  @Column({ type: 'timestamptz', nullable: true })
  apiKeyCreatedAt?: Date;

  @Column({ type: 'timestamptz', nullable: true })
  apiKeyRevokedAt?: Date;

  @Column({
    type: 'enum',
    enum: MerchantStatus,
    default: MerchantStatus.PENDING,
  })
  status: MerchantStatus;

  @CreateDateColumn()
  createdAt: Date;

  @UpdateDateColumn()
  updatedAt: Date;
}
