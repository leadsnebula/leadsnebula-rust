-- Drop legacy tables and schemas
-- This migration removes Rails-specific and test artifacts that are no longer needed

-- Verify ar_internal_metadata is empty before dropping (only if it exists)
DO $$
DECLARE
    table_exists BOOLEAN;
    row_count INTEGER;
BEGIN
    SELECT EXISTS(
        SELECT 1 FROM information_schema.tables
        WHERE table_schema = 'public' AND table_name = 'ar_internal_metadata'
    ) INTO table_exists;

    IF table_exists THEN
        SELECT COUNT(*) INTO row_count FROM ar_internal_metadata;
        IF row_count > 0 THEN
            RAISE WARNING 'ar_internal_metadata contains % rows. Please verify before dropping.', row_count;
        ELSE
            RAISE NOTICE 'ar_internal_metadata is empty. Safe to drop.';
        END IF;
    ELSE
        RAISE NOTICE 'ar_internal_metadata does not exist; skipping verification.';
    END IF;
END $$;

-- Drop ar_internal_metadata table (Rails internal metadata, not needed in Rust app)
DROP TABLE IF EXISTS ar_internal_metadata CASCADE;

-- Verify _sqlx_test schema exists and is empty before dropping
DO $$
DECLARE
    schema_exists BOOLEAN;
    table_count INTEGER;
BEGIN
    SELECT EXISTS(
        SELECT 1 FROM information_schema.schemata WHERE schema_name = '_sqlx_test'
    ) INTO schema_exists;
    
    IF schema_exists THEN
        SELECT COUNT(*) INTO table_count
        FROM information_schema.tables
        WHERE table_schema = '_sqlx_test';
        
        IF table_count > 0 THEN
            RAISE WARNING '_sqlx_test schema contains % tables. Please verify before dropping.', table_count;
        ELSE
            RAISE NOTICE '_sqlx_test schema is empty. Safe to drop.';
        END IF;
    ELSE
        RAISE NOTICE '_sqlx_test schema does not exist.';
    END IF;
END $$;

-- Drop _sqlx_test schema (sqlx test schema, not needed in production)
DROP SCHEMA IF EXISTS _sqlx_test CASCADE;
