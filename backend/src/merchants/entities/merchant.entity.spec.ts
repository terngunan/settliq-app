import { instanceToPlain } from 'class-transformer';
import { Merchant, MerchantRole, MerchantStatus } from './merchant.entity';

describe('Merchant entity — serialization (issue #30)', () => {
  function buildMerchant(): Merchant {
    return Object.assign(new Merchant(), {
      id: 'uuid-1',
      email: 'test@example.com',
      passwordHash: 'bcrypt-hash',
      businessName: 'Test Corp',
      businessType: 'retail',
      country: 'NG',
      status: MerchantStatus.ACTIVE,
      role: MerchantRole.MERCHANT,
      apiKeyHash: 'api-key-hash',
      feeRate: 0.015,
      sandboxMode: false,
      createdAt: new Date('2024-01-01'),
      updatedAt: new Date('2024-01-01'),
      deletedAt: null,
    });
  }

  it('instanceToPlain excludes passwordHash', () => {
    const plain = instanceToPlain(buildMerchant());
    expect(plain.passwordHash).toBeUndefined();
  });

  it('instanceToPlain excludes apiKeyHash', () => {
    const plain = instanceToPlain(buildMerchant());
    expect(plain.apiKeyHash).toBeUndefined();
  });

  it('raw apiKey column no longer exists on the entity class', () => {
    const merchant = buildMerchant();
    // The property should not exist at all — the column was removed in #30.
    expect(Object.prototype.hasOwnProperty.call(merchant, 'apiKey')).toBe(false);
    expect((merchant as any).apiKey).toBeUndefined();
  });

  it('instanceToPlain includes non-sensitive public fields', () => {
    const plain = instanceToPlain(buildMerchant());
    expect(plain.id).toBe('uuid-1');
    expect(plain.email).toBe('test@example.com');
    expect(plain.businessName).toBe('Test Corp');
    expect(plain.status).toBe(MerchantStatus.ACTIVE);
    expect(plain.role).toBe(MerchantRole.MERCHANT);
  });
});

describe('Merchant entity — feeRate numeric transformer (issue #142)', () => {
  function getFeeRateTransformer() {
    const metadata = (Merchant as any).__typeormMetadata__;
    return metadata;
  }

  it('feeRate column defines a transformer that parses DB strings to numbers', () => {
    // Access the column metadata via TypeORM's getMetadataArgsStorage.
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const { getMetadataArgsStorage } = require('typeorm');
    const column = getMetadataArgsStorage()
      .columns.filter((c: any) => c.target === Merchant)
      .find((c: any) => c.propertyName === 'feeRate');

    expect(column).toBeDefined();
    expect(column.options.transformer).toBeDefined();

    const transformer = column.options.transformer;
    const parsed = transformer.from('0.0150');
    expect(typeof parsed).toBe('number');
    expect(parsed).toBe(0.015);

    expect(transformer.to(0.015)).toBe(0.015);
  });

  it('reading feeRate from a DB string yields a number usable in arithmetic', () => {
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const { getMetadataArgsStorage } = require('typeorm');
    const column = getMetadataArgsStorage()
      .columns.filter((c: any) => c.target === Merchant)
      .find((c: any) => c.propertyName === 'feeRate');

    const merchant = Object.assign(new Merchant(), {
      feeRate: column.options.transformer.from('0.0150'),
    });

    expect(typeof merchant.feeRate).toBe('number');
    expect(merchant.feeRate * 100).toBeCloseTo(1.5);
    expect(merchant.feeRate + 0.005).toBeCloseTo(0.02);
  });
});
