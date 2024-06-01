# udp-over-ssh
This is a software for UDP tunneling over ssh with safe packet boundary.


## example

```
# transport LOCAL:12345 to REMOTE:23456 via SERVER 
# you must install udp-over-ssh on CLIENT and SERVER
CLIENT$ mkfifo tmp
CLIENT$ udp-over-ssh listen 12345 < tmp | ssh SERVER udp-over-ssh send REMOTE:23456 > tmp
```
