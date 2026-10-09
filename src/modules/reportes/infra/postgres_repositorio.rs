use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::modules::reportes::domain::{FiltrosConsultaReporte, Reporte, ReporteConsulta};

pub const UMBRAL_COINCIDENCIA_GRADOS: f64 = 0.001;

type FilaReporte = (String, f64, f64, DateTime<Utc>, Option<i16>);

#[derive(Clone)]
pub struct PostgresRepositorioReportes {
    pool: PgPool,
}

impl PostgresRepositorioReportes {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn existe_reporte_reciente(
        &self,
        id_usuario: &str,
        lat: f64,
        lng: f64,
    ) -> Result<bool, sqlx::Error> {
        let encontrado: Option<i32> = sqlx::query_scalar(
            "SELECT 1
             FROM reportes
             WHERE id_usuario = $1
               AND ABS(lat - $2) < $4
               AND ABS(lng - $3) < $4
               AND creado > now() - interval '15 minutes'
             LIMIT 1",
        )
        .bind(id_usuario)
        .bind(lat)
        .bind(lng)
        .bind(UMBRAL_COINCIDENCIA_GRADOS)
        .fetch_optional(&self.pool)
        .await?;
        Ok(encontrado.is_some())
    }

    pub async fn insertar_reporte(
        &self,
        id_usuario: &str,
        lat: f64,
        lng: f64,
        creado: DateTime<Utc>,
        horas_duracion: Option<u16>,
    ) -> Result<Reporte, sqlx::Error> {
        let id: String = sqlx::query_scalar(
            "INSERT INTO reportes (id_usuario, lat, lng, creado, horas_duracion)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id::text",
        )
        .bind(id_usuario)
        .bind(lat)
        .bind(lng)
        .bind(creado)
        .bind(horas_duracion.map(|horas| horas as i16))
        .fetch_one(&self.pool)
        .await?;

        Ok(Reporte {
            id,
            id_usuario: id_usuario.to_owned(),
            lat,
            lng,
            creado,
            horas_duracion,
        })
    }

    pub async fn listar_reportes(
        &self,
        filtros: &FiltrosConsultaReporte,
    ) -> Result<Vec<ReporteConsulta>, sqlx::Error> {
        let bbox = filtros.bbox.as_ref();
        let filas: Vec<FilaReporte> = sqlx::query_as(
            "SELECT id::text, lat, lng, creado, horas_duracion
             FROM reportes
             WHERE creado >= $1
               AND creado < $2
               AND ($3::float8 IS NULL OR lat >= $3)
               AND ($4::float8 IS NULL OR lat <= $4)
               AND ($5::float8 IS NULL OR lng >= $5)
               AND ($6::float8 IS NULL OR lng <= $6)
             ORDER BY creado DESC, id DESC
             LIMIT $7 OFFSET $8",
        )
        .bind(filtros.desde)
        .bind(filtros.hasta)
        .bind(bbox.map(|b| b.min_lat))
        .bind(bbox.map(|b| b.max_lat))
        .bind(bbox.map(|b| b.min_lng))
        .bind(bbox.map(|b| b.max_lng))
        .bind(filtros.limit as i64)
        .bind(filtros.offset as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(filas
            .into_iter()
            .map(|(id, lat, lng, creado, horas_duracion)| ReporteConsulta {
                id,
                lat,
                lng,
                creado,
                horas_duracion: horas_duracion.map(|horas| horas as u16),
            })
            .collect())
    }

    pub async fn contar_reportes(
        &self,
        filtros: &FiltrosConsultaReporte,
    ) -> Result<i64, sqlx::Error> {
        let bbox = filtros.bbox.as_ref();
        let total: i64 = sqlx::query_scalar(
            "SELECT count(*)
             FROM reportes
             WHERE creado >= $1
               AND creado < $2
               AND ($3::float8 IS NULL OR lat >= $3)
               AND ($4::float8 IS NULL OR lat <= $4)
               AND ($5::float8 IS NULL OR lng >= $5)
               AND ($6::float8 IS NULL OR lng <= $6)",
        )
        .bind(filtros.desde)
        .bind(filtros.hasta)
        .bind(bbox.map(|b| b.min_lat))
        .bind(bbox.map(|b| b.max_lat))
        .bind(bbox.map(|b| b.min_lng))
        .bind(bbox.map(|b| b.max_lng))
        .fetch_one(&self.pool)
        .await?;
        Ok(total)
    }
}
