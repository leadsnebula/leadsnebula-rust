-- Add instance_id to password_policy_config and data_processing_agreements
-- These should be per-instance, not global

-- Add instance_id to password_policy_config
ALTER TABLE password_policy_config 
    ADD COLUMN IF NOT EXISTS instance_id UUID;

-- Add instance_id to data_processing_agreements
ALTER TABLE data_processing_agreements 
    ADD COLUMN IF NOT EXISTS instance_id UUID;

-- Add foreign keys
ALTER TABLE password_policy_config 
    ADD CONSTRAINT fk_password_policy_config_instance_id 
    FOREIGN KEY (instance_id) REFERENCES instances(id) ON DELETE CASCADE;

ALTER TABLE data_processing_agreements 
    ADD CONSTRAINT fk_data_processing_agreements_instance_id 
    FOREIGN KEY (instance_id) REFERENCES instances(id) ON DELETE CASCADE;

-- Add indexes
CREATE INDEX IF NOT EXISTS idx_password_policy_config_instance_id ON password_policy_config(instance_id);
CREATE INDEX IF NOT EXISTS idx_data_processing_agreements_instance_id ON data_processing_agreements(instance_id);

-- Update unique constraint on password_policy_config to include instance_id
-- First, drop the old unique constraint if it exists
ALTER TABLE password_policy_config 
    DROP CONSTRAINT IF EXISTS password_policy_config_config_key_key;

-- Add new unique constraint with instance_id
ALTER TABLE password_policy_config 
    ADD CONSTRAINT idx_password_policy_config_key_instance_unique UNIQUE (config_key, instance_id);




