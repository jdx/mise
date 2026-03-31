use super::types::{ProviderInfo, ServiceOffering, ServiceTier};

/// Returns the built-in catalog of available providers and services.
pub fn get_catalog() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo {
            name: "vercel".into(),
            categories: vec!["hosting".into()],
            services: vec![ServiceOffering {
                provider: "vercel".into(),
                service: "project".into(),
                category: "hosting".into(),
                description: "Frontend hosting and serverless functions".into(),
                tiers: vec![
                    ServiceTier {
                        name: "hobby".into(),
                        price: "free".into(),
                        features: vec!["Serverless functions".into(), "Edge network".into()],
                    },
                    ServiceTier {
                        name: "pro".into(),
                        price: "$20/mo".into(),
                        features: vec![
                            "Team collaboration".into(),
                            "Advanced analytics".into(),
                            "Password protection".into(),
                        ],
                    },
                ],
            }],
        },
        ProviderInfo {
            name: "railway".into(),
            categories: vec!["hosting".into(), "database".into(), "storage".into()],
            services: vec![
                ServiceOffering {
                    provider: "railway".into(),
                    service: "project".into(),
                    category: "hosting".into(),
                    description: "Full-stack app hosting with built-in CI/CD".into(),
                    tiers: vec![
                        ServiceTier {
                            name: "trial".into(),
                            price: "free".into(),
                            features: vec!["500 hours/month".into(), "1 GB RAM".into()],
                        },
                        ServiceTier {
                            name: "pro".into(),
                            price: "$5/mo + usage".into(),
                            features: vec!["Unlimited hours".into(), "8 GB RAM".into()],
                        },
                    ],
                },
                ServiceOffering {
                    provider: "railway".into(),
                    service: "database".into(),
                    category: "database".into(),
                    description: "Managed PostgreSQL, MySQL, or Redis".into(),
                    tiers: vec![ServiceTier {
                        name: "standard".into(),
                        price: "usage-based".into(),
                        features: vec!["Auto-scaling".into(), "Daily backups".into()],
                    }],
                },
            ],
        },
        ProviderInfo {
            name: "supabase".into(),
            categories: vec![
                "database".into(),
                "authentication".into(),
                "storage".into(),
            ],
            services: vec![
                ServiceOffering {
                    provider: "supabase".into(),
                    service: "database".into(),
                    category: "database".into(),
                    description: "Managed PostgreSQL with realtime subscriptions".into(),
                    tiers: vec![
                        ServiceTier {
                            name: "free".into(),
                            price: "free".into(),
                            features: vec!["500 MB database".into(), "50k monthly active users".into()],
                        },
                        ServiceTier {
                            name: "pro".into(),
                            price: "$25/mo".into(),
                            features: vec!["8 GB database".into(), "100k monthly active users".into()],
                        },
                    ],
                },
                ServiceOffering {
                    provider: "supabase".into(),
                    service: "auth".into(),
                    category: "authentication".into(),
                    description: "Authentication and user management".into(),
                    tiers: vec![ServiceTier {
                        name: "included".into(),
                        price: "included".into(),
                        features: vec!["Social login".into(), "Row level security".into()],
                    }],
                },
            ],
        },
        ProviderInfo {
            name: "neon".into(),
            categories: vec!["database".into(), "authentication".into()],
            services: vec![ServiceOffering {
                provider: "neon".into(),
                service: "database".into(),
                category: "database".into(),
                description: "Serverless Postgres with branching".into(),
                tiers: vec![
                    ServiceTier {
                        name: "free".into(),
                        price: "free".into(),
                        features: vec!["0.5 GiB storage".into(), "Branching".into()],
                    },
                    ServiceTier {
                        name: "launch".into(),
                        price: "$19/mo".into(),
                        features: vec!["10 GiB storage".into(), "Autoscaling".into()],
                    },
                ],
            }],
        },
        ProviderInfo {
            name: "planetscale".into(),
            categories: vec!["database".into()],
            services: vec![ServiceOffering {
                provider: "planetscale".into(),
                service: "database".into(),
                category: "database".into(),
                description: "Serverless MySQL with branching and deploy requests".into(),
                tiers: vec![
                    ServiceTier {
                        name: "hobby".into(),
                        price: "free".into(),
                        features: vec!["5 GB storage".into(), "1 billion row reads/mo".into()],
                    },
                    ServiceTier {
                        name: "scaler".into(),
                        price: "$29/mo".into(),
                        features: vec![
                            "10 GB storage".into(),
                            "Unlimited connections".into(),
                        ],
                    },
                ],
            }],
        },
        ProviderInfo {
            name: "turso".into(),
            categories: vec!["database".into()],
            services: vec![ServiceOffering {
                provider: "turso".into(),
                service: "database".into(),
                category: "database".into(),
                description: "Edge-hosted distributed SQLite (libSQL)".into(),
                tiers: vec![
                    ServiceTier {
                        name: "starter".into(),
                        price: "free".into(),
                        features: vec!["9 GB storage".into(), "500 databases".into()],
                    },
                    ServiceTier {
                        name: "scaler".into(),
                        price: "$29/mo".into(),
                        features: vec!["24 GB storage".into(), "10k databases".into()],
                    },
                ],
            }],
        },
        ProviderInfo {
            name: "chroma".into(),
            categories: vec!["vector database".into()],
            services: vec![ServiceOffering {
                provider: "chroma".into(),
                service: "database".into(),
                category: "vector database".into(),
                description: "AI-native open-source vector database".into(),
                tiers: vec![ServiceTier {
                    name: "cloud".into(),
                    price: "usage-based".into(),
                    features: vec!["Managed hosting".into(), "Auto-scaling".into()],
                }],
            }],
        },
        ProviderInfo {
            name: "clerk".into(),
            categories: vec!["authentication".into()],
            services: vec![ServiceOffering {
                provider: "clerk".into(),
                service: "auth".into(),
                category: "authentication".into(),
                description: "Drop-in authentication and user management".into(),
                tiers: vec![
                    ServiceTier {
                        name: "free".into(),
                        price: "free".into(),
                        features: vec!["10k monthly active users".into(), "Pre-built components".into()],
                    },
                    ServiceTier {
                        name: "pro".into(),
                        price: "$25/mo".into(),
                        features: vec![
                            "Unlimited MAUs".into(),
                            "Custom domains".into(),
                            "Remove branding".into(),
                        ],
                    },
                ],
            }],
        },
        ProviderInfo {
            name: "posthog".into(),
            categories: vec!["analytics".into(), "feature flags".into()],
            services: vec![ServiceOffering {
                provider: "posthog".into(),
                service: "analytics".into(),
                category: "analytics".into(),
                description: "Product analytics, session replay, and feature flags".into(),
                tiers: vec![
                    ServiceTier {
                        name: "free".into(),
                        price: "free".into(),
                        features: vec!["1M events/mo".into(), "Session replay".into()],
                    },
                    ServiceTier {
                        name: "paid".into(),
                        price: "usage-based".into(),
                        features: vec![
                            "Unlimited events".into(),
                            "Group analytics".into(),
                            "A/B testing".into(),
                        ],
                    },
                ],
            }],
        },
        ProviderInfo {
            name: "runloop".into(),
            categories: vec!["sandboxes".into(), "hosting".into()],
            services: vec![ServiceOffering {
                provider: "runloop".into(),
                service: "sandbox".into(),
                category: "sandboxes".into(),
                description: "Secure sandboxed execution environments".into(),
                tiers: vec![ServiceTier {
                    name: "standard".into(),
                    price: "usage-based".into(),
                    features: vec!["Isolated runtimes".into(), "API access".into()],
                }],
            }],
        },
    ]
}

/// Filter catalog by provider name
pub fn get_provider(name: &str) -> Option<ProviderInfo> {
    get_catalog()
        .into_iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
}

/// Filter catalog by category
pub fn get_by_category(category: &str) -> Vec<ProviderInfo> {
    get_catalog()
        .into_iter()
        .filter(|p| {
            p.categories
                .iter()
                .any(|c| c.eq_ignore_ascii_case(category))
        })
        .collect()
}

/// Find a specific service offering by "provider/service" key
pub fn find_service(provider: &str, service: &str) -> Option<ServiceOffering> {
    get_provider(provider).and_then(|p| {
        p.services
            .into_iter()
            .find(|s| s.service.eq_ignore_ascii_case(service))
    })
}
