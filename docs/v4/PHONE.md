# Kestrel's page on your phone

Through [Tailscale](https://tailscale.com): free for personal use, an account of yours, and an encrypted link
between your own devices. Kestrel stays on the laptop; the phone opens its page.

1. Install Tailscale on this device and on the phone, and sign in with the same account on both.
2. In Kestrel: Settings → Internet → Your phone: on. Or `kestrel settings phone on`.
3. Start Kestrel again. Settings then shows the address to open on the phone. `kestrel open --print --lan` prints it.

## What this is

- The page listens on this device and on its Tailscale address, and on no other network: not the wifi.
- The address holds the key to the page, so keep it to yourself.
- The phone can read and ask. Turning the internet on, the mail account, contacts, feeds, models: only on this
  device (`from_this_device`).
- The link is plain HTTP inside Tailscale's encrypted tunnel. Kestrel adds no encryption of its own.
- The laptop must be on, with Kestrel running.

## What was checked

With the address taken from a stand-in: only a Tailscale address is taken, the page listens there and on
127.0.0.1 and refuses a third, an address that is gone leaves the page on this device, and a change from
another host is refused. **Tailscale itself has not been run with Kestrel**: it is not installed on the machine
this was built on, and installing it needs an account of yours.
