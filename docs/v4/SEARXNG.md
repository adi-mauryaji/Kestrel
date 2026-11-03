# Web search: a SearXNG on this device

Kestrel's web search goes through a SearXNG you choose (ADR-060). It costs nothing. SearXNG asks the search
engines for you, so they see your computer's address and your search, and no account.

**Public instances mostly do not work.** On 2026-11-04 Kestrel tried 25 of the instances searx.space listed as
up: 0 answered a program in JSON (most said 429 or 403, some had no address any more). One on your own device
is the way that works.

## Set it up (Linux, with podman or docker)

About 270 MB, 30 seconds here. Make a folder with this one file, `settings.yml`, with a secret of your own:

```yaml
use_default_settings: true
server:
  secret_key: "put-a-long-random-text-here"
  limiter: false
  image_proxy: false
search:
  formats: [html, json]
```

`formats` must have `json`: Kestrel asks for JSON. Then, from that folder:

```sh
podman run -d --name searxng --restart=always -p 127.0.0.1:8888:8080 \
  -v "$PWD":/etc/searxng:Z docker.io/searxng/searxng:latest
```

`127.0.0.1` keeps it to this device. In Kestrel, Settings → Internet → Web search: `http://127.0.0.1:8888`.
Then ask: `search the web for mango season`. Everything it asks is listed under "What left this device".

To remove it: `podman rm -f searxng`, then Remove in Settings.

## What was checked

The real `kestrel-net`, a real public page and real Gemma, with `experiments/v4/fake_searxng.py` answering in
SearXNG's JSON form. **A real SearXNG has not been run with Kestrel yet**: starting that image was not allowed
in the session that built this. Run the steps above and search once to check it, and say if it does not answer.

On Windows and macOS this needs Docker Desktop or podman, which is heavy for most people; there the card stays
🔧 until there is an address (blueprint §4.8, open item).
