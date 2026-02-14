-- Documentation test instance and publisher for Carina API docs (Scalar).
-- Instance login: info@leadsnebula.com
-- Publisher name: Leads Test
-- Test API key (for X-API-Key in docs): pk_docs_test_LEADSNEBULA
-- Publisher ID (stable for docs): a1b2c3d4-e5f6-4780-a123-456789abcdef

-- 1) Ensure instance_user exists for info@leadsnebula.com
INSERT INTO instance_users (
    id, email, encrypted_password, status, confirmed_at, created_at, updated_at
) VALUES (
    'b2c3d4e5-f6a7-4780-b234-567890abcdef',
    'info@leadsnebula.com',
    '$argon2id$v=19$m=19456,t=2,p=1$placeholder$placeholder',
    'active',
    NOW(),
    NOW(),
    NOW()
) ON CONFLICT (email) DO NOTHING;

-- 2) Ensure instance exists (Leads Test Instance), owned by that user
INSERT INTO instances (
    id, name, instance_user_id, payment_status, created_at, updated_at
) VALUES (
    'c3d4e5f6-a7b8-4780-c345-678901abcdef',
    'Leads Test Instance',
    'b2c3d4e5-f6a7-4780-b234-567890abcdef',
    'active',
    NOW(),
    NOW()
) ON CONFLICT (id) DO NOTHING;

-- If instance_user was already present, link instance to them by email
UPDATE instances
SET instance_user_id = (SELECT id FROM instance_users WHERE email = 'info@leadsnebula.com' LIMIT 1)
WHERE id = 'c3d4e5f6-a7b8-4780-c345-678901abcdef'
  AND instance_user_id IS NULL;

-- 3) Insert publisher "Leads Test" with fixed test API key (hash = SHA256(pk_docs_test_LEADSNEBULA))
INSERT INTO publishers (
    id, name, email, api_key_hash, api_key_prefix, api_key_encrypted, status,
    instance_id, is_documentation_test, created_at, updated_at
) VALUES (
    'a1b2c3d4-e5f6-4780-a123-456789abcdef',
    'Leads Test',
    'leads-test@leadsnebula.com',
    encode(digest('pk_docs_test_LEADSNEBULA', 'sha256'), 'hex'),
    'pk_docs_test_',
    '',
    'active',
    'c3d4e5f6-a7b8-4780-c345-678901abcdef',
    true,
    NOW(),
    NOW()
) ON CONFLICT (id) DO UPDATE SET
    name = EXCLUDED.name,
    api_key_hash = EXCLUDED.api_key_hash,
    api_key_prefix = EXCLUDED.api_key_prefix,
    is_documentation_test = true,
    updated_at = NOW();

-- 4) Link publisher to solar vertical
INSERT INTO publisher_verticals (publisher_id, vertical_id)
SELECT 'a1b2c3d4-e5f6-4780-a123-456789abcdef', id FROM verticals WHERE slug = 'solar' AND is_active = true LIMIT 1
ON CONFLICT (publisher_id, vertical_id) DO NOTHING;
