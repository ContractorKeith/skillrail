#!/usr/bin/env python3
"""Force simultaneous resize/input readiness through the real TUI and PTY driver."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
from smoke import seed, tui_smoke

# This controls only legal readiness batching/order. It neither changes input
# bytes nor fakes application events. The old edge-triggered reader loses the
# input event after returning a signal-first batch. The level-triggered reader
# must see both descriptors ready and preserve progress without a second key.
SOURCE = r"""
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <poll.h>
#include <sys/epoll.h>
#include <unistd.h>
#include <stdlib.h>
#include <stdio.h>
static const char *active(void) {
 const char *gate=getenv("SKILLRAIL_PTY_POLL_GATE");
 return gate && access(gate,F_OK)==0 ? gate : NULL;
}
static void seen(const char *gate,const char *kind) {
 char path[4096];snprintf(path,sizeof(path),"%s.seen",gate);
 FILE *out=fopen(path,"a");if(out){fprintf(out,"%s\n",kind);fclose(out);}
}
int epoll_wait(int fd,struct epoll_event *ev,int max,int timeout) {
 int (*wait)(int,struct epoll_event*,int,int)=dlsym(RTLD_NEXT,"epoll_wait");
 int n=wait(fd,ev,max,timeout);const char *gate=active();
 if(n>0&&gate){
  int tty=0,signal=0;
  for(int i=0;i<n;i++){tty|=ev[i].data.u64==0;signal|=ev[i].data.u64==1;}
  if(!(tty&&signal)&&n<max){
   // Parent has committed to sending the single quit. Wait for its readiness,
   // rather than assuming the scheduler will run it within an arbitrary delay.
   int m;do{m=wait(fd,ev+n,max-n,-1);}while(m<0&&errno==EINTR);
   if(m>0)n+=m;
  }
  tty=signal=0;
  for(int i=0;i<n;i++){tty|=ev[i].data.u64==0;signal|=ev[i].data.u64==1;}
  // Pinned Crossterm 0.29 tokens: TTY=0, SIGWINCH=1. Both orders are legal.
  for(int i=0;i<n;i++)for(int j=i+1;j<n;j++)if(ev[j].data.u64>ev[i].data.u64){struct epoll_event tmp=ev[i];ev[i]=ev[j];ev[j]=tmp;}
  if(tty&&signal)seen(gate,"epoll signal+tty");
 }
 return n;
}
int poll(struct pollfd *fds,nfds_t count,int timeout) {
 int (*wait)(struct pollfd*,nfds_t,int)=dlsym(RTLD_NEXT,"poll");
 int n=wait(fds,count,timeout);const char *gate=active();
 if(n>0&&gate&&count==2){
  // Coalesce arrivals by waiting only on the descriptor not ready yet, then
  // obtain an ordinary level-triggered snapshot of the original descriptors.
  for(nfds_t i=0;i<count;i++)if(!(fds[i].revents&POLLIN)){
   struct pollfd pending=fds[i];pending.revents=0;
   int ready;do{ready=wait(&pending,1,-1);}while(ready<0&&errno==EINTR);
  }
  n=wait(fds,count,0);
  if((fds[0].revents&POLLIN)&&(fds[1].revents&POLLIN))seen(gate,"poll tty+signal");
 }
 return n;
}
"""


def main():
    if sys.platform != "linux":
        print("PTY readiness: deterministic syscall-order probe is Linux-only; portable resize/input smoke runs on this platform")
        return
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"]))
    binary = Path(metadata["target_directory"]) / "debug/skillrail"
    with tempfile.TemporaryDirectory(prefix="pty-ready-") as directory:
        root = Path(directory).resolve()
        (root / "home").mkdir()
        (root / "project").mkdir()
        seed(root)
        source = root / "order.c"
        library = root / "order.so"
        source.write_text(SOURCE)
        subprocess.run(["cc", "-shared", "-fPIC", str(source), "-ldl", "-o", str(library)],
                       check=True, capture_output=True)
        gate = root / "gate"

        def before_quit(process):
            gate.touch()
            os.kill(process.pid, signal.SIGWINCH)

        environment = dict(os.environ, LD_PRELOAD=str(library),
                           SKILLRAIL_PTY_POLL_GATE=str(gate))
        tui_smoke(binary, root, process_env=environment, before_quit=before_quit)
        evidence = (root / "gate.seen").read_text()
        assert any(batch in evidence for batch in ("poll tty+signal", "epoll signal+tty")), f"simultaneous readiness control did not fire: {evidence!r}"
        print("PTY readiness: simultaneous resize/input retained quit and restored terminal")


if __name__ == "__main__":
    main()
