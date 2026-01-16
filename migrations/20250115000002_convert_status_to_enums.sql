-- Convert status columns to ENUM types for better performance and data integrity
-- This migration converts high-frequency status columns incrementally

-- ============================================
-- STEP 1: Create ENUM types
-- ============================================

-- Lead status enum (based on CHECK constraint in schema)
DO $$ BEGIN
    CREATE TYPE lead_status_enum AS ENUM (
        'processing',
        'ping_accepted',
        'sold',
        'rejected',
        'timeout',
        'invalid',
        'error'
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- Ping result enum (common values from ping responses)
DO $$ BEGIN
    CREATE TYPE ping_result_enum AS ENUM (
        'accepted',
        'rejected',
        'timeout',
        'invalid',
        'error',
        'sold'  -- Used when ping leads to a sale
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- Post result enum (common values from post responses)
DO $$ BEGIN
    CREATE TYPE post_result_enum AS ENUM (
        'sold',
        'rejected',
        'timeout',
        'invalid',
        'error'
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- Publisher status enum (common values)
DO $$ BEGIN
    CREATE TYPE publisher_status_enum AS ENUM (
        'active',
        'inactive',
        'suspended'
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- Buyer status enum (common values)
DO $$ BEGIN
    CREATE TYPE buyer_status_enum AS ENUM (
        'active',
        'incomplete',
        'inactive',
        'suspended'
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- Campaign status enum (common values)
DO $$ BEGIN
    CREATE TYPE campaign_status_enum AS ENUM (
        'active',
        'paused',
        'inactive'
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- Instance user status enum (common values)
DO $$ BEGIN
    CREATE TYPE instance_user_status_enum AS ENUM (
        'active',
        'inactive',
        'suspended'
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- ============================================
-- STEP 2: Convert leads.status (HIGH FREQUENCY)
-- ============================================

-- Add new column with ENUM type (idempotent)
DO $$ BEGIN
    ALTER TABLE leads ADD COLUMN status_new lead_status_enum;
EXCEPTION
    WHEN duplicate_column THEN null;
END $$;

-- Migrate data (handle any invalid values by defaulting to 'error')
UPDATE leads SET status_new = CASE
    WHEN status = 'processing' THEN 'processing'::lead_status_enum
    WHEN status = 'ping_accepted' THEN 'ping_accepted'::lead_status_enum
    WHEN status = 'sold' THEN 'sold'::lead_status_enum
    WHEN status = 'rejected' THEN 'rejected'::lead_status_enum
    WHEN status = 'timeout' THEN 'timeout'::lead_status_enum
    WHEN status = 'invalid' THEN 'invalid'::lead_status_enum
    WHEN status = 'error' THEN 'error'::lead_status_enum
    ELSE 'error'::lead_status_enum  -- Default invalid values to 'error'
END;

-- Make new column NOT NULL (idempotent - only if column exists)
DO $$ BEGIN
    ALTER TABLE leads ALTER COLUMN status_new SET NOT NULL;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;
DO $$ BEGIN
    ALTER TABLE leads ALTER COLUMN status_new SET DEFAULT 'processing'::lead_status_enum;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;

-- Drop old column and rename new one (idempotent)
DO $$ BEGIN
    ALTER TABLE leads DROP COLUMN status;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;
DO $$ BEGIN
    ALTER TABLE leads RENAME COLUMN status_new TO status;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;

-- ============================================
-- STEP 3: Convert pings.result (HIGH FREQUENCY)
-- ============================================

-- Check if pings table has result column
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'pings' AND column_name = 'result'
    ) THEN
        -- Add new column with ENUM type (idempotent)
        BEGIN
            ALTER TABLE pings ADD COLUMN result_new ping_result_enum;
        EXCEPTION
            WHEN duplicate_column THEN null;
        END;
        
        -- Migrate data
        UPDATE pings SET result_new = CASE
            WHEN result = 'accepted' THEN 'accepted'::ping_result_enum
            WHEN result = 'rejected' THEN 'rejected'::ping_result_enum
            WHEN result = 'timeout' THEN 'timeout'::ping_result_enum
            WHEN result = 'invalid' THEN 'invalid'::ping_result_enum
            WHEN result = 'error' THEN 'error'::ping_result_enum
            WHEN result = 'sold' THEN 'sold'::ping_result_enum
            ELSE NULL  -- Allow NULL for unknown values
        END;
        
        -- Drop old column and rename new one (idempotent)
        BEGIN
            ALTER TABLE pings DROP COLUMN result;
        EXCEPTION
            WHEN undefined_column THEN null;
        END;
        BEGIN
            ALTER TABLE pings RENAME COLUMN result_new TO result;
        EXCEPTION
            WHEN undefined_column THEN null;
        END;
    END IF;
END $$;

-- ============================================
-- STEP 4: Convert posts.result (HIGH FREQUENCY)
-- ============================================

-- Check if posts table has result column
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'posts' AND column_name = 'result'
    ) THEN
        -- Add new column with ENUM type (idempotent)
        BEGIN
            ALTER TABLE posts ADD COLUMN result_new post_result_enum;
        EXCEPTION
            WHEN duplicate_column THEN null;
        END;
        
        -- Migrate data
        UPDATE posts SET result_new = CASE
            WHEN result = 'sold' THEN 'sold'::post_result_enum
            WHEN result = 'rejected' THEN 'rejected'::post_result_enum
            WHEN result = 'timeout' THEN 'timeout'::post_result_enum
            WHEN result = 'invalid' THEN 'invalid'::post_result_enum
            WHEN result = 'error' THEN 'error'::post_result_enum
            ELSE NULL  -- Allow NULL for unknown values
        END;
        
        -- Drop old column and rename new one (idempotent)
        BEGIN
            ALTER TABLE posts DROP COLUMN result;
        EXCEPTION
            WHEN undefined_column THEN null;
        END;
        BEGIN
            ALTER TABLE posts RENAME COLUMN result_new TO result;
        EXCEPTION
            WHEN undefined_column THEN null;
        END;
    END IF;
END $$;

-- ============================================
-- STEP 5: Convert publishers.status (MEDIUM FREQUENCY)
-- ============================================

-- Add new column with ENUM type (idempotent)
DO $$ BEGIN
    ALTER TABLE publishers ADD COLUMN status_new publisher_status_enum;
EXCEPTION
    WHEN duplicate_column THEN null;
END $$;

-- Migrate data (default 'inactive' for unknown values)
UPDATE publishers SET status_new = CASE
    WHEN status = 'active' THEN 'active'::publisher_status_enum
    WHEN status = 'inactive' THEN 'inactive'::publisher_status_enum
    WHEN status = 'suspended' THEN 'suspended'::publisher_status_enum
    ELSE 'inactive'::publisher_status_enum  -- Default unknown values
END;

-- Make new column NOT NULL (idempotent)
DO $$ BEGIN
    ALTER TABLE publishers ALTER COLUMN status_new SET NOT NULL;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;
DO $$ BEGIN
    ALTER TABLE publishers ALTER COLUMN status_new SET DEFAULT 'active'::publisher_status_enum;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;

-- Drop old column and rename new one (idempotent)
DO $$ BEGIN
    ALTER TABLE publishers DROP COLUMN status;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;
DO $$ BEGIN
    ALTER TABLE publishers RENAME COLUMN status_new TO status;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;

-- ============================================
-- STEP 6: Convert buyers.status (MEDIUM FREQUENCY)
-- ============================================

-- Add new column with ENUM type (idempotent)
DO $$ BEGIN
    ALTER TABLE buyers ADD COLUMN status_new buyer_status_enum;
EXCEPTION
    WHEN duplicate_column THEN null;
END $$;

-- Migrate data (preserve 'incomplete' as default)
UPDATE buyers SET status_new = CASE
    WHEN status = 'active' THEN 'active'::buyer_status_enum
    WHEN status = 'incomplete' THEN 'incomplete'::buyer_status_enum
    WHEN status = 'inactive' THEN 'inactive'::buyer_status_enum
    WHEN status = 'suspended' THEN 'suspended'::buyer_status_enum
    ELSE 'incomplete'::buyer_status_enum  -- Default unknown values
END;

-- Make new column NOT NULL (idempotent)
DO $$ BEGIN
    ALTER TABLE buyers ALTER COLUMN status_new SET NOT NULL;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;
DO $$ BEGIN
    ALTER TABLE buyers ALTER COLUMN status_new SET DEFAULT 'incomplete'::buyer_status_enum;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;

-- Drop old column and rename new one (idempotent)
DO $$ BEGIN
    ALTER TABLE buyers DROP COLUMN status;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;
DO $$ BEGIN
    ALTER TABLE buyers RENAME COLUMN status_new TO status;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;

-- ============================================
-- STEP 7: Convert campaigns.status (MEDIUM FREQUENCY)
-- ============================================

-- Add new column with ENUM type (idempotent)
DO $$ BEGIN
    ALTER TABLE campaigns ADD COLUMN status_new campaign_status_enum;
EXCEPTION
    WHEN duplicate_column THEN null;
END $$;

-- Migrate data
UPDATE campaigns SET status_new = CASE
    WHEN status = 'active' THEN 'active'::campaign_status_enum
    WHEN status = 'paused' THEN 'paused'::campaign_status_enum
    WHEN status = 'inactive' THEN 'inactive'::campaign_status_enum
    ELSE 'active'::campaign_status_enum  -- Default unknown values
END;

-- Make new column NOT NULL (idempotent)
DO $$ BEGIN
    ALTER TABLE campaigns ALTER COLUMN status_new SET NOT NULL;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;
DO $$ BEGIN
    ALTER TABLE campaigns ALTER COLUMN status_new SET DEFAULT 'active'::campaign_status_enum;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;

-- Drop old column and rename new one (idempotent)
DO $$ BEGIN
    ALTER TABLE campaigns DROP COLUMN status;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;
DO $$ BEGIN
    ALTER TABLE campaigns RENAME COLUMN status_new TO status;
EXCEPTION
    WHEN undefined_column THEN null;
END $$;

-- ============================================
-- STEP 8: Convert instance_users.status (LOW FREQUENCY)
-- ============================================

-- Check if instance_users table has status column
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'instance_users' AND column_name = 'status'
    ) THEN
        -- Add new column with ENUM type (idempotent)
        BEGIN
            ALTER TABLE instance_users ADD COLUMN status_new instance_user_status_enum;
        EXCEPTION
            WHEN duplicate_column THEN null;
        END;
        
        -- Migrate data
        UPDATE instance_users SET status_new = CASE
            WHEN status = 'active' THEN 'active'::instance_user_status_enum
            WHEN status = 'inactive' THEN 'inactive'::instance_user_status_enum
            WHEN status = 'suspended' THEN 'suspended'::instance_user_status_enum
            ELSE 'active'::instance_user_status_enum  -- Default unknown values
        END;
        
        -- Make new column NOT NULL (idempotent)
        BEGIN
            ALTER TABLE instance_users ALTER COLUMN status_new SET NOT NULL;
        EXCEPTION
            WHEN undefined_column THEN null;
        END;
        BEGIN
            ALTER TABLE instance_users ALTER COLUMN status_new SET DEFAULT 'active'::instance_user_status_enum;
        EXCEPTION
            WHEN undefined_column THEN null;
        END;
        
        -- Drop old column and rename new one (idempotent)
        BEGIN
            ALTER TABLE instance_users DROP COLUMN status;
        EXCEPTION
            WHEN undefined_column THEN null;
        END;
        BEGIN
            ALTER TABLE instance_users RENAME COLUMN status_new TO status;
        EXCEPTION
            WHEN undefined_column THEN null;
        END;
    END IF;
END $$;

-- ============================================
-- NOTES:
-- ============================================
-- 1. ping_trees.status already has a CHECK constraint - consider converting later
-- 2. instances.payment_status already has a CHECK constraint - consider converting later
-- 3. All ENUM types are created but can be extended if new values are needed:
--    ALTER TYPE lead_status_enum ADD VALUE 'new_status';
-- 4. Indexes on status columns will continue to work with ENUMs
-- 5. Application code should be updated to use ENUM types in Rust models
