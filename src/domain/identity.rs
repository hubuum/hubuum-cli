use hubuum_client::{
    MeResponse, PrincipalCollectionPermissions, PrincipalTokenMetadata, ServiceAccount,
};

transparent_record!(MeRecord, MeResponse);
transparent_record!(PrincipalTokenRecord, PrincipalTokenMetadata);
transparent_record!(PrincipalPermissionsRecord, PrincipalCollectionPermissions);
transparent_record!(ServiceAccountRecord, ServiceAccount);
