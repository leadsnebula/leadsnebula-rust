-- Set correct login emails for the two standard instances (dev/setup).
-- API Lead Test instance (c3d4e5f6) -> user b2c3d4e5 -> info@leadsnebula.com
-- Leads Nebula instance (147b486c) -> user d6bf3245 -> boris@leadsnebula.com
-- Passwords are unchanged. Run cleanup-dev-instances-and-users.sql on dev DB for full cleanup.
-- Use temp email to avoid unique constraint when both users currently have the same email.

UPDATE instance_users
SET email = 'leadsnebula-instance-user-d6bf3245@temp.local', updated_at = NOW()
WHERE id = 'd6bf3245-833d-4176-954b-edfa08e5edb5'::uuid;

UPDATE instance_users
SET email = 'leadsnebula-instance-user-b2c3d4e5@temp.local', updated_at = NOW()
WHERE id = 'b2c3d4e5-f6a7-4780-b234-567890abcdef'::uuid;

UPDATE instance_users
SET email = 'boris@leadsnebula.com', updated_at = NOW()
WHERE id = 'd6bf3245-833d-4176-954b-edfa08e5edb5'::uuid;

UPDATE instance_users
SET email = 'info@leadsnebula.com', updated_at = NOW()
WHERE id = 'b2c3d4e5-f6a7-4780-b234-567890abcdef'::uuid;
