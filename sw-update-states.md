
# SW Updates

```mermaid
---
title: SW Update state diagram
---
stateDiagram-v2
    idle : Idle
    swu : Install SW Update
    uboot : U-Boot
    mark_failed : U-Boot recovery
    cur_slot : Current slot verification

    [*] --> idle : Fresh Install

    idle --> swu : OTA update detected

    swu --> uboot : Install success | ustate = 1, bootpart = X, boot_count = 0

    uboot --> cur_slot : ustate != 1 || boot_count < MAX_TRIES

    uboot --> mark_failed : ustate == 1 && boot_count >= MAX_TRIES

    mark_failed --> idle : true | bootpart = alt(X), ustate = 3

    cur_slot --> uboot : (!verified || wdt reboot) | boot_count += 1

    cur_slot --> idle : verified | ustate = 0
```

 Corrected state machine

 ```mermaid
 stateDiagram-v2
     idle     : Idle (confirmed)
     swu      : Installing (SWUpdate)
     uboot    : U-Boot arbitration
     trial    : Trial boot (unconfirmed)
     fallback : U-Boot fallback

     [*] --> idle : Fresh install | ustate=0, bootcount=0

     idle --> swu   : OTA offered && ustate == 0
     swu  --> idle  : install / signature failure | ustate=3
     swu  --> uboot : install success | bootenv{bootpart=standby, bootcount=0, ustate=1}

     uboot --> fallback : ustate == 1 && bootcount >= bootlimit
     uboot --> trial    : ustate == 1 | bootcount += 1, saveenv
     uboot --> idle     : ustate != 1

     trial --> idle  : health check passed | ustate=0, bootcount=0
     trial --> uboot : check failed / panic / hang / WDT / power loss

     fallback --> uboot : bootpart=alt(bootpart), ustate=3, bootcount=0
```
