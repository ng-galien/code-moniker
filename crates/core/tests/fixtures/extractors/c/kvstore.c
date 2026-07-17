#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "kvstore.h"

// cm: def kv entry struct
typedef struct kventry {
	char *key;
	void *value;
	// cm: def next field
	// cm: ref next field typed as kventry
	// cm: ref entry struct field uses its own type
	struct kventry *next;
} kventry;

// cm: def kv store struct
typedef struct kvstore {
	kventry *head;
	unsigned long size;
	// cm: def free method pointer field
	void (*free_value)(void *value);
} kvstore;

static unsigned long kv_hash(const char *key) {
	unsigned long hash = 5381;
	int c;
	while ((c = *key++))
		hash = ((hash << 5) + hash) + c;
	return hash;
}

// cm: def kv create
kvstore *kv_create(void) {
	// cm: ref create calls libc malloc
	kvstore *store = malloc(sizeof(kvstore));
	store->head = NULL;
	store->size = 0;
	return store;
}

// cm: def kv insert
int kv_insert(kvstore *store, const char *key, void *value) {
	kventry *entry = malloc(sizeof(kventry));
	// cm: ref insert calls libc strdup
	entry->key = strdup(key);
	entry->value = value;
	entry->next = store->head;
	store->head = entry;
	store->size++;
	// cm: ref insert calls static hash
	return (int)(kv_hash(key) % 16);
}

// cm: def kv destroy
void kv_destroy(kvstore *store) {
	kventry *entry = store->head;
	while (entry) {
		kventry *next = entry->next;
		if (store->free_value) {
			// cm: ref typed field pointer call binds the field
			store->free_value(entry->value);
		}
		free(entry->key);
		free(entry);
		entry = next;
	}
	free(store);
}

// cm: def chain through return type
unsigned long kv_first_hash(void) {
	// cm: ref chained receiver via kv_create return
	return kv_create()->size;
}
