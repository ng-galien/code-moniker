#include "postgres.h"
#include "local_generated.h"

static int local_helper(void) {
	return 1;
}

int run(void) {
	Oid oid = 0;
	RequestAddinShmemSpace(sizeof(Oid));
	return local_helper() + POSTGRES_CONST + (int) oid;
}
