-- Set Leads Test Instance (info@leadsnebula.com) owner so that user is instance admin.
-- Run from rust/: ./scripts/run-set-instance-admin-info.sh
-- Or: psql "$DATABASE_URL" -f scripts/set-instance-admin-info.sql

UPDATE instances
SET instance_user_id = (SELECT id FROM instance_users WHERE LOWER(email) = 'info@leadsnebula.com' LIMIT 1),
    updated_at = NOW()
WHERE id = 'c3d4e5f6-a7b8-4780-c345-678901abcdef';
