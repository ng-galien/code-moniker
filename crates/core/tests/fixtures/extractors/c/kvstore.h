#ifndef KVSTORE_H
#define KVSTORE_H

// cm: def opaque store typedef
typedef struct kvstore kvstore;

// cm: def create prototype
kvstore *kv_create(void);
// cm: def insert prototype
int kv_insert(kvstore *store, const char *key, void *value);
void kv_destroy(kvstore *store);

// cm: def exported counter
extern unsigned long kv_total_inserts;

#endif
