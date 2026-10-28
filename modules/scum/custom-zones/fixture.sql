-- Synthetic schema-73 fixture. No game assets, accounts or world records.
PRAGMA user_version=73;
PRAGMA journal_mode=WAL;
CREATE TABLE map(id INTEGER PRIMARY KEY);
CREATE TABLE custom_zone_configuration(
  id INTEGER NOT NULL, map_id INTEGER NOT NULL, name CHAR(100),
  color_red REAL, color_green REAL, color_blue REAL,
  handling_methods BIGINT, settings INTEGER, user_id TEXT DEFAULT '',
  num_handling_methods INTEGER NOT NULL DEFAULT 0, future_column TEXT,
  PRIMARY KEY(id,map_id), FOREIGN KEY(map_id) REFERENCES map(id) ON DELETE CASCADE
);
CREATE TABLE custom_zone_region(
  id INTEGER NOT NULL, map_id INTEGER NOT NULL, name CHAR(100),
  location_x REAL, location_y REAL, size_x REAL, size_y REAL,
  configuration_index TINYINT, default_region_name CHAR(50),
  default_region_state TINYINT, user_id TEXT DEFAULT '', future_column TEXT,
  PRIMARY KEY(id,map_id), FOREIGN KEY(map_id) REFERENCES map(id) ON DELETE CASCADE
);
CREATE TABLE custom_zone_configuration_damage_handling_methods(
  id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  custom_zone_configuration_id INTEGER NOT NULL, map_id INTEGER NOT NULL,
  damage_actor_type INTEGER, damage_handling_methods INTEGER,
  num_damage_handling_methods INTEGER NOT NULL DEFAULT 0, future_column BLOB,
  FOREIGN KEY(custom_zone_configuration_id,map_id)
    REFERENCES custom_zone_configuration(id,map_id) ON DELETE CASCADE
);
CREATE TABLE retained_world(id INTEGER PRIMARY KEY, unknown_data BLOB);
INSERT INTO map VALUES(1),(2);
INSERT INTO custom_zone_configuration VALUES
  (1,1,'Workshop',0.1,0.2,0.3,42,3,'synthetic-owner',15,'preserve-config'),
  (2,1,'Harbour',0.4,0.5,0.6,21,2,'',15,'preserve-second-config'),
  (1000,1,'Global',0.7,0.8,0.9,42,1,'',15,'preserve-global'),
  (1,2,'Other map',0.2,0.2,0.2,21,0,'',15,'preserve-other-map');
INSERT INTO custom_zone_region VALUES
  (1,1,'Workshop north',123.5,-456.25,500,300,0,'default-outpost',1,'','preserve-region'),
  (2,1,'Workshop south',-120,250,200,100,0,'',0,'','preserve-second-region'),
  (3,1,'Harbour pier',30,40,100,0,1,'',0,'','preserve-other-region');
INSERT INTO custom_zone_configuration_damage_handling_methods VALUES
  (1,1,1,5,36028797018964309,5,X'000102FF'),
  (2,1,1,1,357913941,15,X'112233'),
  (3,2,1,5,341,5,X'445566'),
  (4,1000,1,5,341,5,X'778899'),
  (5,1,2,5,341,5,X'AABBCC');
INSERT INTO retained_world VALUES(1,X'0001FEFF'),(2,X'55667788');
