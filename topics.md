# Topics worth considering

- mDNS sucks cause it gives up another host when there are "conflicts" This seems to happen even when there are no actual conflicts i can see.
  Probably due to a simple reboot of the OS before the DNS records could expire.
  - Write my own resolver (high prio).
  - Cloud rendezvous. This is: Proxying devices via an internet service. (could be interesting. does not fit fully-offline situations)
- HTTPS currently not supported:
  - We could actually support it using self-signed certs or by using dns challenges. The cert gets 
    embedded into the device.
    - Downsides: Need to renew certs by updating SW, requires DNS server to point to the right IP.

