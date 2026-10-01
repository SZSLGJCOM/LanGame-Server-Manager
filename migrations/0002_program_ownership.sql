-- Existing instances own physical runtime copies. Reject unfamiliar layouts
-- before changing their associations; SQLx applies this migration atomically.
CREATE TABLE program_ownership_path_check (
    valid INTEGER NOT NULL
        CONSTRAINT instance_config_path_must_end_in_config CHECK (valid = 1)
);
INSERT INTO program_ownership_path_check (valid)
SELECT CASE
    WHEN length(config_path) > 7
        AND lower(substr(config_path, -7)) IN ('/config', '\config')
        AND instr(config_path, char(0)) = 0
        AND (substr(replace(config_path, '\', '/'), 1, 1) = '/'
            OR replace(config_path, '\', '/') GLOB '[A-Za-z]:/*')
        AND instr(replace(config_path, '\', '/'), '/../') = 0
        AND instr(replace(config_path, '\', '/'), '/./') = 0
    THEN 1 ELSE 0 END
FROM instances;
INSERT INTO program_ownership_path_check (valid)
SELECT CASE WHEN COUNT(*) = 0 THEN 1 ELSE 0 END
FROM (
    SELECT lower(replace(config_path, '\', '/')) AS normalized_config_path
    FROM instances GROUP BY normalized_config_path HAVING COUNT(*) > 1
);
DROP TABLE program_ownership_path_check;

ALTER TABLE game_installs ADD COLUMN scope TEXT NOT NULL DEFAULT 'library'
    CHECK (scope IN ('library', 'instance'));
ALTER TABLE game_installs ADD COLUMN owner_instance_id TEXT
    REFERENCES instances(id) ON DELETE CASCADE
    CHECK ((scope = 'library' AND owner_instance_id IS NULL)
        OR (scope = 'instance' AND owner_instance_id IS NOT NULL));
ALTER TABLE instances ADD COLUMN runtime_mode TEXT NOT NULL DEFAULT 'independent'
    CHECK (runtime_mode IN ('independent', 'shared'));

CREATE UNIQUE INDEX idx_game_installs_instance_owner
    ON game_installs(owner_instance_id) WHERE scope = 'instance';
CREATE INDEX idx_game_installs_scope_module
    ON game_installs(scope, module_id, install_state);

INSERT INTO game_installs (
    module_id, install_root, install_state, current_version, last_verified_at,
    created_at, updated_at, scope, owner_instance_id
)
SELECT instance.module_id,
       substr(instance.config_path, 1, length(instance.config_path) - 6) || 'runtime',
       'incomplete', NULL, NULL, instance.created_at, instance.updated_at,
       'instance', instance.id
FROM instances AS instance;

UPDATE instances
SET install_id = (
    SELECT id FROM game_installs
    WHERE scope = 'instance' AND owner_instance_id = instances.id
);

PRAGMA user_version = 2;
