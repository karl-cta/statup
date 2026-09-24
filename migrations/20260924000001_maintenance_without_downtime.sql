-- Work that does not take its services down: they keep their state while it
-- runs, and the status banner stays calm.
ALTER TABLE events ADD COLUMN keeps_services_up INTEGER NOT NULL DEFAULT 0
    CHECK (keeps_services_up IN (0, 1));
