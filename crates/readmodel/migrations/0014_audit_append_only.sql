-- The audit log is append-only (security review, 12 November 2026).
--
-- A row is written before its command is submitted, then completed exactly once: with the
-- command's sequence number and events, or with the error. After that nothing may change it,
-- and no row may ever be deleted or the table truncated. The application's own role still owns
-- the table, so a holder of that role could drop this trigger; it stops application bugs and
-- injected statements, not a stolen database password (docs/threat-model.md).
CREATE FUNCTION audit_log_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'UPDATE' THEN
        RAISE EXCEPTION 'audit_log is append-only: % refused', TG_OP;
    END IF;
    IF OLD.seq IS NOT NULL OR OLD.detail ? 'events' OR OLD.detail ? 'error' THEN
        RAISE EXCEPTION 'audit_log row % is complete and cannot change', OLD.id;
    END IF;
    IF NEW.id <> OLD.id OR NEW.at <> OLD.at OR NEW.user_id IS DISTINCT FROM OLD.user_id
       OR NEW.actor <> OLD.actor OR NEW.action <> OLD.action OR NEW.reason <> OLD.reason
       OR NOT (NEW.detail @> OLD.detail) THEN
        RAISE EXCEPTION 'audit_log row % may only gain its outcome', OLD.id;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER audit_log_append_only
    BEFORE UPDATE OR DELETE ON audit_log
    FOR EACH ROW EXECUTE FUNCTION audit_log_guard();

CREATE TRIGGER audit_log_no_truncate
    BEFORE TRUNCATE ON audit_log
    FOR EACH STATEMENT EXECUTE FUNCTION audit_log_guard();
