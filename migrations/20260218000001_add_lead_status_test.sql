-- Add 'test' to lead_status_enum for leads submitted with is_test = true.
-- Test leads: do not count toward revenue; exclude from reporting.
DO $$ BEGIN
    ALTER TYPE lead_status_enum ADD VALUE 'test';
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;
