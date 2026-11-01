#include <signal.h>
#include <unistd.h>
static void h(int s){ (void)s; _exit(139); }
__attribute__((constructor)) static void init(void){
  struct sigaction sa; sa.sa_handler=h; sigemptyset(&sa.sa_mask); sa.sa_flags=0;
  sigaction(SIGSEGV,&sa,0); sigaction(SIGABRT,&sa,0); sigaction(SIGBUS,&sa,0);
  sigaction(SIGILL,&sa,0); sigaction(SIGFPE,&sa,0);
}
