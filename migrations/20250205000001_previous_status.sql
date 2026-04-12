-- Add previous_status column to track the last status before a transition,
-- enabling one-level undo on status changes.
ALTER TABLE events ADD COLUMN previous_status TEXT;
