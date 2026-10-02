# photo-transformer-service

Microsserviço Rust que:

1. Lista e baixa imagens de uma pasta do **Google Drive**
2. Redimensiona para **1200×1200 JPEG** no modo **contain** (imagem inteira, centrada em fundo branco `#FFFFFF`, qualidade ~90) — igual ao app web `index.html`
3. Localiza o produto no **Bling API v3** pelo `codigo`/SKU = nome do arquivo sem extensão
4. Anexa a mídia no produto via `PATCH /produtos/{id}` com `midia.imagens.imagensURL`

## Endpoints HTTP (Axum)

| Método | Rota | Descrição |
|--------|------|-----------|
| `GET` | `/health` | Healthcheck |
| `POST` | `/sync` ou `/jobs` | Dispara Drive → processar → Bling (async) |
| `GET` | `/status` | Status do último/ atual job |
| `GET` | `/media/{id}.jpg` | JPEG processado (URL pública para o Bling baixar) |

Se `SYNC_API_KEY` estiver definido, envie `X-Api-Key` ou `Authorization: Bearer <key>` no `POST /sync`.

Body opcional do sync:

```json
{ "folder_id": "ID_DA_PASTA_DRIVE" }
```

## Variáveis de ambiente

Copie `.env.example` para `.env` e preencha. **Não commite segredos.**

### Obrigatórias

| Variável | Uso |
|----------|-----|
| `GOOGLE_DRIVE_FOLDER_ID` | Pasta de origem no Drive |
| `BLING_CLIENT_ID` / `BLING_CLIENT_SECRET` / `BLING_REFRESH_TOKEN` | OAuth2 Bling |
| Auth Google | **Service account** (`GOOGLE_SERVICE_ACCOUNT_JSON`) **ou** OAuth (`GOOGLE_CLIENT_ID` + `GOOGLE_CLIENT_SECRET` + `GOOGLE_REFRESH_TOKEN`) |

### Importantes

| Variável | Uso |
|----------|-----|
| `PUBLIC_BASE_URL` | URL pública deste serviço (ex.: `https://fotos.seudominio.com`). O Bling **só aceita imagem por URL**; o serviço serve `/media/...` para o Bling baixar. |
| `GOOGLE_SUPPORTS_ALL_DRIVES` | `true` para Shared Drives / pastas compartilhadas |
| `GOOGLE_DRIVE_OUTPUT_FOLDER_ID` | Opcional: grava o JPEG 1200×1200 no Drive |
| `SYNC_API_KEY` | Protege `POST /sync` |
| `JPEG_QUALITY` | Padrão `90` |
| `HOST` / `PORT` | Bind HTTP (padrão `0.0.0.0:8080`) |

## Setup Google Drive

### Opção A — Service account (recomendado no VPS)

1. No [Google Cloud Console](https://console.cloud.google.com/), crie um projeto e ative a **Google Drive API**.
2. Crie uma **Service Account**, baixe o JSON.
3. Compartilhe a pasta do Drive (e o Shared Drive, se houver) com o e-mail da service account (`...@....iam.gserviceaccount.com`) como **Editor** (ou Leitor + pasta de saída separada).
4. Defina `GOOGLE_SERVICE_ACCOUNT_JSON=/run/secrets/google-sa.json` e monte o arquivo no container.

### Opção B — OAuth (usuário)

1. Crie OAuth Client (tipo Desktop ou Web) no Cloud Console.
2. Autorize o escopo `https://www.googleapis.com/auth/drive` e obtenha `refresh_token`.
3. Preencha `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `GOOGLE_REFRESH_TOKEN`.

## Setup Bling OAuth2 (API v3)

1. Acesse [Bling Developers](https://developer.bling.com.br/) e cadastre um aplicativo.
2. Fluxo de autorização: o usuário autoriza e você troca o `authorization_code` por tokens em  
   `POST https://api.bling.com.br/Api/v3/oauth/token`  
   com header `Authorization: Basic base64(client_id:client_secret)` e body `grant_type=authorization_code&code=...&redirect_uri=...`.
3. Guarde o `refresh_token` em `BLING_REFRESH_TOKEN`. O serviço renova o Bearer automaticamente (`grant_type=refresh_token`).
4. Escopos necessários: leitura/escrita de **produtos** (midia).

### Premissas de endpoints Bling usadas neste serviço

- Base: `https://api.bling.com.br/Api/v3`
- Token: `POST /oauth/token` (Basic + form-urlencoded)
- Busca produto: `GET /produtos?codigos[]={SKU}&criterio=5`
- Anexa imagem: `PATCH /produtos/{id}` com:

```json
{
  "midia": {
    "video": { "url": "" },
    "imagens": {
      "imagensURL": [{ "link": "https://seu-servico/media/....jpg" }]
    }
  }
}
```

(Conforme OpenAPI oficial: `imagensURL` é write-only; o Bling baixa a URL.)

**Matching:** nome do arquivo no Drive sem extensão = `codigo` do produto (SKU). Ex.: `SKU-001.jpg` → produto `SKU-001`.

## Rodar localmente

```bash
cd service
cp .env.example .env
# edite .env
cargo run --release
curl -s localhost:8080/health
curl -s -X POST localhost:8080/sync -H "X-Api-Key: $SYNC_API_KEY"
curl -s localhost:8080/status
```

## Deploy no VPS (Docker)

```bash
cd service
cp .env.example .env
# edite .env — PUBLIC_BASE_URL deve ser a URL pública (nginx/Caddy → :8080)
mkdir -p credentials
# copie o JSON da service account para credentials/google-sa.json
export GOOGLE_SA_HOST_PATH=./credentials/google-sa.json

docker compose up -d --build
docker compose logs -f
```

Exponha com reverse proxy (TLS) apontando para a porta `8080`. Sem `PUBLIC_BASE_URL` alcançável pela internet, o Bling não consegue baixar as imagens.

### Exemplo Caddy

```
fotos.seudominio.com {
  reverse_proxy 127.0.0.1:8080
}
```

## Estrutura

```
service/
  Cargo.toml
  Dockerfile
  docker-compose.yml
  .env.example
  README.md
  src/
    main.rs
    api.rs
    config.rs
    drive.rs
    bling.rs
    image_proc.rs
    sync.rs
    error.rs
```

O app web na raiz do repositório (`index.html`) permanece intacto.
