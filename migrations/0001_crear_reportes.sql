CREATE TABLE reportes (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    id_usuario     TEXT NOT NULL,
    lat            DOUBLE PRECISION NOT NULL,
    lng            DOUBLE PRECISION NOT NULL,
    creado         TIMESTAMPTZ NOT NULL DEFAULT now(),
    horas_duracion SMALLINT NULL,
    CHECK (lat BETWEEN -90 AND 90),
    CHECK (lng BETWEEN -180 AND 180),
    CHECK (horas_duracion IS NULL OR horas_duracion BETWEEN 0 AND 168)
);

CREATE INDEX idx_reportes_id_usuario ON reportes (id_usuario);
CREATE INDEX idx_reportes_creado ON reportes (creado);