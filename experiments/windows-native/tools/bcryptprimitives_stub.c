/* Wine 8.0 does not ship bcryptprimitives.dll (ProcessPrng); forward it to
   bcrypt.dll so the mingw-built DBM binary can start under Wine for testing. */
#include <windows.h>
#include <bcrypt.h>

__declspec(dllexport) BOOL ProcessPrng(PBYTE pbData, SIZE_T cbData) {
    return BCryptGenRandom(NULL, pbData, (ULONG)cbData, BCRYPT_USE_SYSTEM_PREFERRED_RNG) == 0;
}
