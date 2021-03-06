CREATE TABLE IF NOT EXISTS public.encrypted_data_keys
(
    id bigint NOT NULL GENERATED BY DEFAULT AS IDENTITY ( INCREMENT 1 START 1 MINVALUE 1 MAXVALUE 9223372036854775807 CACHE 1 ),
    key_desc character varying(100) COLLATE pg_catalog."default",
    data_key bytea NOT NULL,
    cmk_name character varying(100) COLLATE pg_catalog."default",
    CONSTRAINT encrypted_data_keys_pkey PRIMARY KEY (id)
)

TABLESPACE pg_default;

ALTER TABLE IF EXISTS public.encrypted_data_keys
    OWNER to postgres;

COMMENT ON COLUMN public.encrypted_data_keys.key_desc
    IS 'description of CMK key';
    
CREATE TABLE IF NOT EXISTS public.data
(
    id integer NOT NULL GENERATED BY DEFAULT AS IDENTITY ( INCREMENT 1 START 1 MINVALUE 1 MAXVALUE 2147483647 CACHE 1 ),
    user_name character(50) COLLATE pg_catalog."default" NOT NULL,
    data_key_id bigint NOT NULL,
    sensitive_data bytea NOT NULL,
    CONSTRAINT data_pkey PRIMARY KEY (id),
    CONSTRAINT encrypted_data_keys_fk FOREIGN KEY (data_key_id)
        REFERENCES public.encrypted_data_keys (id) MATCH SIMPLE
        ON UPDATE NO ACTION
        ON DELETE NO ACTION
        NOT VALID
)

TABLESPACE pg_default;

ALTER TABLE IF EXISTS public.data
    OWNER to postgres;