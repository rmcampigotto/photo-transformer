# photo-transformer

App web mínimo: corta o centro da foto e redimensiona para **1200×1200**. Só HTML — sem build, sem backend.

## Uso

1. **Upload** — escolha uma foto (galeria ou arquivos).
2. Aguarde a barra de progresso.
3. **Download** — salva o JPEG 1200×1200.

## Abrir no iPhone (Safari)

**Servidor local (recomendado)** — no computador, na pasta do app:

```bash
cd photo-transformer
python3 -m http.server 8080
```

No iPhone (mesma Wi‑Fi), abra no Safari: `http://IP-DO-COMPUTADOR:8080`

**Arquivo direto:** copie `index.html` para o iPhone e abra pelo app Arquivos → Safari. Se a galeria não abrir via `file://`, use o servidor acima.

## Limitações

- Uma foto por vez.
- Saída sempre JPEG (qualidade ~0,92).
- HEIC depende do Safari; em outros navegadores pode falhar.
- Orientação EXIF via `createImageBitmap` (Safari/Chrome modernos).

## Arquivos

```
photo-transformer/
├── index.html
└── README.md
```
