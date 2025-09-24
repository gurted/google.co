CREATE TYPE domain_status AS ENUM ('pending', 'ready', 'blocked', 'error');

CREATE TYPE fetch_outcome AS ENUM ('pending', 'success', 'redirect', 'error');

CREATE TABLE domains (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    submission_source TEXT,
    submitted_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    status domain_status NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    first_crawled_at TIMESTAMPTZ,
    last_crawled_at TIMESTAMPTZ,
    crawl_interval_seconds INTEGER,
    robots_checksum BYTEA,
    robots_crawl_delay_ms INTEGER,
    robots_checked_at TIMESTAMPTZ,
    robots_etag TEXT,
    robots_last_status SMALLINT,
    UNIQUE (name),
    CHECK (crawl_interval_seconds IS NULL OR crawl_interval_seconds >= 0),
    CHECK (robots_crawl_delay_ms IS NULL OR robots_crawl_delay_ms >= 0)
);

CREATE UNIQUE INDEX idx_domains_name_ci ON domains ((LOWER(name)));

CREATE TABLE urls (
    id BIGSERIAL PRIMARY KEY,
    domain_id BIGINT NOT NULL REFERENCES domains(id) ON DELETE CASCADE,
    canonical_url TEXT NOT NULL,
    normalized_hash BYTEA NOT NULL,
    fetch_priority INTEGER NOT NULL DEFAULT 0,
    discovered_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_crawled_at TIMESTAMPTZ,
    last_fetch_status SMALLINT,
    last_fetch_outcome fetch_outcome NOT NULL DEFAULT 'pending',
    last_fetch_error TEXT,
    last_content_hash BYTEA,
    last_crawl_duration_ms INTEGER,
    etag TEXT,
    last_modified TIMESTAMPTZ,
    content_type TEXT,
    content_length BIGINT,
    robots_blocked BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (canonical_url),
    UNIQUE (domain_id, normalized_hash),
    CHECK (fetch_priority >= 0),
    CHECK (last_crawl_duration_ms IS NULL OR last_crawl_duration_ms >= 0),
    CHECK (content_length IS NULL OR content_length >= 0)
);

CREATE INDEX idx_urls_domain ON urls (domain_id);
CREATE INDEX idx_urls_domain_priority ON urls (domain_id, fetch_priority DESC, discovered_at);
CREATE INDEX idx_urls_last_crawled ON urls (last_crawled_at);

CREATE TABLE crawl_queue (
    id BIGSERIAL PRIMARY KEY,
    url_id BIGINT NOT NULL REFERENCES urls(id) ON DELETE CASCADE,
    domain_id BIGINT NOT NULL REFERENCES domains(id) ON DELETE CASCADE,
    available_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    priority INTEGER NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 5,
    locked_by TEXT,
    locked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (url_id),
    CHECK (priority >= 0),
    CHECK (attempts >= 0),
    CHECK (max_attempts > 0)
);

CREATE INDEX idx_crawl_queue_ready ON crawl_queue (priority DESC, available_at) WHERE locked_by IS NULL;
CREATE INDEX idx_crawl_queue_domain ON crawl_queue (domain_id, available_at);
CREATE INDEX idx_crawl_queue_locked_by ON crawl_queue (locked_by) WHERE locked_by IS NOT NULL;

CREATE TABLE recrawl_queue (
    id BIGSERIAL PRIMARY KEY,
    url_id BIGINT NOT NULL REFERENCES urls(id) ON DELETE CASCADE,
    domain_id BIGINT NOT NULL REFERENCES domains(id) ON DELETE CASCADE,
    available_at TIMESTAMPTZ NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    recrawl_interval_seconds INTEGER,
    locked_by TEXT,
    locked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (url_id),
    CHECK (priority >= 0),
    CHECK (attempts >= 0),
    CHECK (recrawl_interval_seconds IS NULL OR recrawl_interval_seconds > 0)
);

CREATE INDEX idx_recrawl_queue_ready ON recrawl_queue (priority DESC, available_at) WHERE locked_by IS NULL;
CREATE INDEX idx_recrawl_queue_domain ON recrawl_queue (domain_id, available_at);
CREATE INDEX idx_recrawl_queue_locked_by ON recrawl_queue (locked_by) WHERE locked_by IS NOT NULL;

CREATE TABLE robots_cache (
    domain_id BIGINT PRIMARY KEY REFERENCES domains(id) ON DELETE CASCADE,
    body TEXT,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMPTZ,
    etag TEXT,
    checksum BYTEA,
    status_code SMALLINT,
    content_length BIGINT,
    last_error TEXT,
    CHECK (content_length IS NULL OR content_length >= 0)
);

CREATE INDEX idx_robots_cache_expiry ON robots_cache (expires_at);

CREATE TABLE fetch_history (
    id BIGSERIAL PRIMARY KEY,
    url_id BIGINT NOT NULL REFERENCES urls(id) ON DELETE CASCADE,
    domain_id BIGINT NOT NULL REFERENCES domains(id) ON DELETE CASCADE,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    status_code INTEGER,
    outcome fetch_outcome NOT NULL DEFAULT 'pending',
    content_length BIGINT,
    content_hash BYTEA,
    error TEXT,
    latency_ms INTEGER,
    response_headers JSONB,
    request_headers JSONB,
    worker_id TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    CHECK (content_length IS NULL OR content_length >= 0),
    CHECK (latency_ms IS NULL OR latency_ms >= 0),
    CHECK (retry_count >= 0)
);

CREATE INDEX idx_fetch_history_url ON fetch_history (url_id, fetched_at DESC);
CREATE INDEX idx_fetch_history_domain ON fetch_history (domain_id, fetched_at DESC);

CREATE TABLE link_edges (
    src_url_id BIGINT NOT NULL REFERENCES urls(id) ON DELETE CASCADE,
    dst_url_id BIGINT NOT NULL REFERENCES urls(id) ON DELETE CASCADE,
    edge_type SMALLINT NOT NULL DEFAULT 0,
    discovered_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    anchor_text TEXT,
    PRIMARY KEY (src_url_id, dst_url_id, edge_type)
);

CREATE INDEX idx_link_edges_src ON link_edges (src_url_id);
CREATE INDEX idx_link_edges_dst ON link_edges (dst_url_id);

CREATE TABLE link_authority (
    url_id BIGINT PRIMARY KEY REFERENCES urls(id) ON DELETE CASCADE,
    domain_id BIGINT NOT NULL REFERENCES domains(id) ON DELETE CASCADE,
    page_rank DOUBLE PRECISION NOT NULL DEFAULT 0,
    trust_rank DOUBLE PRECISION NOT NULL DEFAULT 0,
    inbound_links INTEGER NOT NULL DEFAULT 0,
    outbound_links INTEGER NOT NULL DEFAULT 0,
    score DOUBLE PRECISION NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (inbound_links >= 0),
    CHECK (outbound_links >= 0)
);

CREATE INDEX idx_link_authority_domain ON link_authority (domain_id);
CREATE INDEX idx_link_authority_score ON link_authority (score DESC);

CREATE TABLE index_segments (
    id BIGSERIAL PRIMARY KEY,
    segment_id TEXT NOT NULL,
    domain_id BIGINT REFERENCES domains(id) ON DELETE SET NULL,
    url_id BIGINT REFERENCES urls(id) ON DELETE SET NULL,
    commit_generation BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    published_at TIMESTAMPTZ,
    deleted_at TIMESTAMPTZ,
    doc_count INTEGER,
    byte_size BIGINT,
    UNIQUE (segment_id),
    CHECK (doc_count IS NULL OR doc_count >= 0),
    CHECK (byte_size IS NULL OR byte_size >= 0)
);

CREATE INDEX idx_index_segments_generation ON index_segments (commit_generation DESC, created_at DESC);
CREATE INDEX idx_index_segments_domain ON index_segments (domain_id);
CREATE INDEX idx_index_segments_url ON index_segments (url_id);

CREATE TABLE query_cache (
    id BIGSERIAL PRIMARY KEY,
    query_hash BYTEA NOT NULL UNIQUE,
    query_text TEXT NOT NULL,
    params JSONB,
    result JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_accessed_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMPTZ,
    hit_count INTEGER NOT NULL DEFAULT 0,
    CHECK (hit_count >= 0)
);

CREATE INDEX idx_query_cache_expires ON query_cache (expires_at);
CREATE INDEX idx_query_cache_last_accessed ON query_cache (last_accessed_at DESC);

CREATE TABLE rate_limit (
    domain_id BIGINT PRIMARY KEY REFERENCES domains(id) ON DELETE CASCADE,
    limit_per_second DOUBLE PRECISION NOT NULL,
    burst_capacity INTEGER NOT NULL,
    tokens_remaining DOUBLE PRECISION NOT NULL,
    last_refill_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at TIMESTAMPTZ,
    locked_by TEXT,
    locked_at TIMESTAMPTZ,
    CHECK (limit_per_second >= 0),
    CHECK (burst_capacity >= 0),
    CHECK (tokens_remaining >= 0)
);

CREATE INDEX idx_rate_limit_locked ON rate_limit (locked_by) WHERE locked_by IS NOT NULL;